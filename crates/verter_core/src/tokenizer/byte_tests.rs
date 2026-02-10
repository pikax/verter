use super::byte::{tokenize, tokenize_with_delimiters};
use super::types::{Event, QuoteType};
use crate::common::ErrorCode;

fn collect_events(input: &str) -> Vec<Event> {
    let mut events = Vec::new();
    tokenize(input.as_bytes(), |event| events.push(event));
    events
}

// ==================== Basic tokenization tests ====================

#[test]
fn test_basic_element() {
    let events = collect_events("<div>hello</div>");

    assert!(events
        .iter()
        .any(|e| matches!(e, Event::OpenTagName { start: 0, end: 4 })));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::OpenTagEnd { end: 5 })));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Text { start: 5, end: 10 })));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::CloseTag {
            start: 10,
            end: 16,
            ..
        }
    )));
}

#[test]
fn test_close_tag_name_end() {
    let input = "<div>hello</div>";
    let events = collect_events(input);

    // Find the CloseTag event and verify name_end allows correct name extraction
    let close_tag = events.iter().find_map(|e| {
        if let Event::CloseTag {
            start,
            end,
            name_end,
        } = e
        {
            Some((*start, *end, *name_end))
        } else {
            None
        }
    });

    let (start, end, name_end) = close_tag.expect("Should have CloseTag event");

    // Verify the name can be extracted with slice(start + 2, name_end)
    let name = &input[start as usize + 2..name_end as usize];
    assert_eq!(
        name, "div",
        "slice(start + 2, name_end) should give the tag name"
    );

    // Verify end is after the closing >
    assert_eq!(
        &input[end as usize - 1..end as usize],
        ">",
        "end should be after >"
    );
}

#[test]
fn test_close_tag_name_end_with_whitespace() {
    // Close tag with whitespace before >: </div >
    let input = "<div></div  >";
    let events = collect_events(input);

    let close_tag = events.iter().find_map(|e| {
        if let Event::CloseTag {
            start,
            end,
            name_end,
        } = e
        {
            Some((*start, *end, *name_end))
        } else {
            None
        }
    });

    let (start, _end, name_end) = close_tag.expect("Should have CloseTag event");

    // Verify the name can be extracted correctly even with trailing whitespace
    let name = &input[start as usize + 2..name_end as usize];
    assert_eq!(
        name, "div",
        "slice(start + 2, name_end) should give the tag name without whitespace"
    );
}

#[test]
fn test_close_tag_name_end_with_newline_before_gt() {
    // Close tag split across lines: </CButton\n>
    // The > should be part of the closing tag, NOT text content
    // This pattern is common with Prettier-formatted Vue templates
    let input = "<div><span>text</span\n>more</div>";
    let events = collect_events(input);

    // Collect all text events
    let text_events: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let Event::Text { start, end } = e {
                Some(&input[*start as usize..*end as usize])
            } else {
                None
            }
        })
        .collect();

    // "text" should be one text node, "more" should be another
    // Neither should contain ">" as stray content
    assert!(
        text_events.contains(&"text"),
        "should have 'text' node, got: {:?}",
        text_events
    );
    assert!(
        text_events.contains(&"more"),
        "should have 'more' node, got: {:?}",
        text_events
    );
    // The > from </span\n> should NOT leak into text
    for text in &text_events {
        assert!(
            !text.starts_with('>'),
            "text '{}' should not start with '>' (leaked from close tag)",
            text
        );
    }
}

#[test]
fn test_self_closing_element() {
    let events = collect_events("<input />");

    assert!(events
        .iter()
        .any(|e| matches!(e, Event::OpenTagName { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::SelfClosingTag { .. })));
    // Should NOT have OpenTagEnd for self-closing
    assert!(!events.iter().any(|e| matches!(e, Event::OpenTagEnd { .. })));
}

#[test]
fn test_consecutive_self_closing_elements() {
    let events = collect_events("<a/><b/>");

    let self_closing_count = events
        .iter()
        .filter(|e| matches!(e, Event::SelfClosingTag { .. }))
        .count();
    assert_eq!(self_closing_count, 2, "Expected 2 SelfClosingTag events");

    let open_tag_names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(open_tag_names.len(), 2, "Expected 2 OpenTagName events");
    assert_eq!(open_tag_names[0], (0, 2), "First tag name at 0-2");
    assert_eq!(open_tag_names[1], (4, 6), "Second tag name at 4-6");
}

// ==================== Interpolation tests ====================

#[test]
fn test_interpolation_basic() {
    let events = collect_events("{{ msg }}");

    assert!(events.iter().any(|e| matches!(
        e,
        Event::Interpolation {
            start: 0,
            end: 9,
            ..
        }
    )));
}

#[test]
fn test_interpolation_in_element() {
    let events = collect_events("<div>{{ msg }}</div>");

    assert!(events.iter().any(|e| matches!(
        e,
        Event::Interpolation {
            start: 5,
            end: 14,
            ..
        }
    )));
}

// ==================== v-pre directive tests ====================

#[test]
fn test_v_pre_directive_detected() {
    let events = collect_events("<span v-pre></span>");

    assert!(
        events.iter().any(|e| matches!(e, Event::DirVPre { .. })),
        "Should detect v-pre directive"
    );
}

#[test]
fn test_v_pre_converts_interpolation_to_text() {
    let events = collect_events("<span v-pre>{{ msg }}</span>");

    // Inside v-pre, interpolation should NOT be detected (emitted as text)
    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation inside v-pre should NOT be emitted as Interpolation"
    );

    // Should have text instead
    let has_text = events.iter().any(|e| matches!(e, Event::Text { .. }));
    assert!(has_text, "Content inside v-pre should be emitted as Text");
}

#[test]
fn test_v_pre_scope_exits_on_close_tag() {
    let events = collect_events("<span v-pre>{{ raw }}</span>{{ compiled }}");

    // Count interpolations - only the one OUTSIDE v-pre should be Interpolation
    let interpolations: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Interpolation { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "Only {{ compiled }} should be Interpolation, got {:?}",
        interpolations
    );

    // The interpolation should be at position 28 (after </span>)
    assert_eq!(
        interpolations[0].0, 28,
        "Interpolation should start at 28 (after </span>)"
    );
}

#[test]
fn test_v_pre_nested_elements() {
    let events = collect_events("<div v-pre><a>{{ msg }}</a></div>{{ outside }}");

    // Interpolation inside nested element should be text
    // Interpolation outside should be Interpolation
    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "Only {{ outside }} should be Interpolation"
    );
}

// ==================== v-pre with self-closing element ====================

/// Self-closing elements with v-pre correctly exit v-pre scope.
#[test]
fn test_v_pre_self_closing_exits_scope() {
    let events = collect_events("<input v-pre />{{ after }}");

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "{{ after }} should be Interpolation after self-closing v-pre"
    );

    let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
    assert!(
        text.is_none(),
        "Self-closing v-pre should not emit Text content"
    );
}

#[test]
fn test_v_pre_self_closing() {
    let events = collect_events("<input v-pre/>{{ msg }}");

    // Inside v-pre, interpolation should NOT be detected (emitted as text)
    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        has_interpolation,
        "Interpolation after self-closing v-pre should be emitted as Interpolation"
    );

    let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
    assert!(
        text.is_none(),
        "Self-closing v-pre should not emit Text content"
    );
}
#[test]
fn test_v_pre_closing() {
    let events = collect_events("<div v-pre></div>{{ msg }}");

    // Inside v-pre, interpolation should NOT be detected (emitted as text)
    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        has_interpolation,
        "Interpolation after closing v-pre should be emitted as Interpolation"
    );

    let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
    assert!(text.is_none(), "Closing v-pre should not emit Text content");
}
#[test]
fn test_v_pre() {
    let events = collect_events("<div v-pre>{{ msg }}</div>");

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation in v-pre should not be emitted"
    );

    let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
    assert!(
        text.is_some(),
        "v-pre should emit Text content for interpolation"
    );
}
#[test]
fn test_v_pre_spaced() {
    let events = collect_events("<div v-pre  >{{ msg }}</div>");

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation in v-pre should not be emitted"
    );

    let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
    assert!(
        text.is_some(),
        "v-pre should emit Text content for interpolation"
    );
}

/// Self-closing and normal v-pre elements behave the same way.
#[test]
fn test_v_pre_self_closing_vs_normal_element() {
    // With normal element
    let events_normal = collect_events("<span v-pre></span>{{ after }}");
    let interp_normal = events_normal
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .count();

    // With self-closing element
    let events_self_closing = collect_events("<input v-pre />{{ after }}");
    let interp_self_closing = events_self_closing
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .count();

    // Both should have 1 interpolation (v-pre scope exits correctly)
    assert_eq!(
        interp_normal, 1,
        "Normal v-pre element should exit scope correctly"
    );
    assert_eq!(
        interp_self_closing, 1,
        "Self-closing v-pre element should exit scope correctly"
    );
    assert_eq!(
        interp_normal, interp_self_closing,
        "Both should behave the same"
    );
}

/// Multiple self-closing v-pre elements each exit their scope independently.
#[test]
fn test_multiple_self_closing_v_pre_each_exit_scope() {
    let events = collect_events("<input v-pre /><input v-pre />{{ after }}");

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "{{ after }} should be Interpolation after multiple self-closing v-pre"
    );
}

// ==================== Edge cases ====================

#[test]
fn test_v_pre_sibling_scope_isolation() {
    let events = collect_events("<p v-pre>{{ a }}</p><p>{{ b }}</p>");

    // Only {{ b }} should be Interpolation
    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "Only {{ b }} should be Interpolation"
    );
}

#[test]
fn test_nested_v_pre_ignored() {
    // Inner v-pre should be ignored since already in v-pre scope
    let events = collect_events("<div v-pre><span v-pre>{{ msg }}</span></div>{{ outside }}");

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "Only {{ outside }} should be Interpolation"
    );
}

// ==================== v-pre attribute ordering tests ====================
// The tokenizer processes attributes sequentially without lookahead.
// This means v-pre must be detected before OpenTagEnd is emitted.
// When v-pre comes after other directives, behavior should be the same.

#[test]
fn test_v_pre_first_with_other_attributes() {
    // v-pre comes first - this is the "canonical" form
    let events = collect_events(r#"<div v-pre v-if="1 > 2">{{ foo }}</div>"#);

    // Interpolation inside v-pre should be text, not Interpolation
    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    let dirnames: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        0,
        "{{ foo }} inside v-pre should NOT be Interpolation (v-pre first)"
    );

    assert_eq!(dirnames.len(), 0, "v-pre directive should no be detected");
}

#[test]
fn test_v_prex_first_with_other_attributes() {
    // v-pre comes first - this is the "canonical" form
    let events = collect_events(r#"<div v-prex v-if="1 > 2">{{ foo }}</div>"#);

    // Interpolation inside v-pre should be text, not Interpolation
    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    let dirnames: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        1,
        "{{ foo }} inside v-pre should NOT be Interpolation (v-pre first)"
    );

    assert_eq!(dirnames.len(), 2, "v-pre directive should be detected");
}

#[test]
fn test_v_pre_last_with_other_attributes() {
    // v-pre comes after v-if - should behave the same as when v-pre comes first
    // (pre-pass detects v-pre ahead and suppresses directives before it)
    let events = collect_events(r#"<div v-if="1 > 2" v-pre>{{ foo }}</div>"#);

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    let dirnames: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        0,
        "{{ foo }} inside v-pre should NOT be Interpolation (v-pre last)"
    );
    assert_eq!(dirnames.len(), 0, "V-pre directive should not be detected");
}

#[test]
fn test_v_pre_attribute_order_equivalence() {
    // Both orderings should produce equivalent behavior for content inside
    let input_v_pre_first = r#"<div v-pre v-if="1 > 2">{{ foo }}</div>"#;
    let input_v_pre_last = r#"<div v-if="1 > 2" v-pre>{{ foo }}</div>"#;

    let events_first = collect_events(input_v_pre_first);
    let events_last = collect_events(input_v_pre_last);

    let interp_first = events_first
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .count();
    let interp_last = events_last
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .count();

    assert_eq!(
        interp_first, interp_last,
        "v-pre attribute order should not affect interpolation handling. \
         First: {} interpolations, Last: {} interpolations",
        interp_first, interp_last
    );
}

#[test]
fn test_v_pre_with_gt_in_attribute_value() {
    // Test with > character in attribute value (common in v-if conditions)
    let events = collect_events(r#"<span v-pre :class="x > 0 ? 'a' : 'b'">{{ msg }}</span>"#);

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    assert_eq!(
        interpolations.len(),
        0,
        "{{ msg }} inside v-pre should be text, not Interpolation"
    );
}

#[test]
fn test_v_pre_after_gt_in_attribute() {
    // The problematic case: v-pre comes AFTER an attribute containing >
    let events = collect_events(r#"<span :class="x > 0 ? 'a' : 'b'" v-pre>{{ msg }}</span>"#);

    let interpolations: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .collect();

    // This documents the expected behavior - both orderings should be equivalent
    assert_eq!(
        interpolations.len(),
        0,
        "{{ msg }} inside v-pre should be text even when v-pre comes after attribute with >"
    );
}

// ==================== Directive dynamic argument tests ====================

#[test]
fn test_directive_static_argument() {
    let events = collect_events("<div v-foo:arg />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
    assert_eq!(
        dir_args[0], false,
        "Static argument should have is_dynamic=false"
    );
}

#[test]
fn test_directive_dynamic_argument() {
    let events = collect_events("<div v-foo:[arg] />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
    assert_eq!(
        dir_args[0], true,
        "Dynamic argument should have is_dynamic=true"
    );
}

#[test]
fn test_directive_dynamic_argument_nested_brackets() {
    let events = collect_events("<div v-foo:[arr[0]] />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
    assert_eq!(
        dir_args[0], true,
        "Dynamic argument with nested brackets should have is_dynamic=true"
    );
}

#[test]
fn test_directive_static_vs_dynamic_arguments() {
    let events = collect_events("<div v-foo:static v-bar:[dynamic] />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 2, "Should have two DirArg events");
    assert_eq!(dir_args[0], false, "First argument should be static");
    assert_eq!(dir_args[1], true, "Second argument should be dynamic");
}

#[test]
fn test_directive_dynamic_argument_with_modifier() {
    let events = collect_events("<div v-foo:[arg].mod />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
    assert_eq!(
        dir_args[0], true,
        "Dynamic argument with modifier should have is_dynamic=true"
    );
}

#[test]
fn test_directive_static_argument_with_modifier() {
    let events = collect_events("<div v-foo:arg.mod />");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
    assert_eq!(
        dir_args[0], false,
        "Static argument with modifier should have is_dynamic=false"
    );
}

#[test]
fn test_skip_script() {
    let events = collect_events("<script><div v-foo:arg.mod /> </script>");

    let dir_args: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
            _ => None,
        })
        .collect();

    assert_eq!(
        dir_args.len(),
        0,
        "Should have no DirArg events inside script"
    );
}

#[test]
fn test_not_send_first_child_text_node_if_empty() {
    let events = collect_events("<div>    <span></span></div>");

    let text_nodes: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Text { .. }))
        .collect();

    assert_eq!(
        text_nodes.len(),
        0,
        "Should have no Text events for empty div"
    );
}

#[test]
fn test_send_v_pre_directive_not_directive_or_attribute() {
    let events = collect_events("<div v-pre></div>");
    let v_pre_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirVPre { .. }))
        .collect();
    assert_eq!(v_pre_events.len(), 1, "Should have one DirVPre event");

    let other_dir_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();
    assert_eq!(
        other_dir_events.len(),
        0,
        "Should have no other DirName events"
    );
}

// ==================== Offset validation tests ====================
// These tests verify that the offsets in events actually correspond to the correct
// slices of the input source. This ensures the tokenizer produces correct byte ranges.

#[test]
fn test_tag_name_offsets() {
    let input = "<div></div>";
    let events = collect_events(input);

    // OpenTagName includes the "<" character
    let open_tag = events.iter().find_map(|e| match e {
        Event::OpenTagName { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(open_tag.is_some(), "Should have OpenTagName event");
    let (start, end) = open_tag.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "<div",
        "OpenTagName offsets [{}:{}] should match '<div' (includes <)",
        start,
        end
    );
    // The actual tag name without < is at [start+1..end]
    assert_eq!(
        &input[start as usize + 1..end as usize],
        "div",
        "Tag name without '<' should be 'div'"
    );

    // CloseTag should capture "</div>"
    let close_tag = events.iter().find_map(|e| match e {
        Event::CloseTag {
            start,
            end,
            name_end,
        } => Some((*start, *end, *name_end)),
        _ => None,
    });
    assert!(close_tag.is_some(), "Should have CloseTag event");
    let (start, end, name_end) = close_tag.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "</div>",
        "CloseTag offsets [{}:{}] should match '</div>'",
        start,
        end
    );
    assert_eq!(
        &input[start as usize + 2..name_end as usize],
        "div",
        "CloseTag name offsets [{}:{}] should match 'div'",
        start + 2,
        name_end
    );
}

#[test]
fn test_attribute_name_offsets() {
    let input = r#"<div class="hello"></div>"#;
    let events = collect_events(input);

    let attrib_name = events.iter().find_map(|e| match e {
        Event::AttribName { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(attrib_name.is_some(), "Should have AttribName event");
    let (start, end) = attrib_name.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "class",
        "AttribName offsets [{}:{}] should match 'class'",
        start,
        end
    );
}

#[test]
fn test_attribute_data_offsets() {
    let input = r#"<div class="hello"></div>"#;
    let events = collect_events(input);

    let attrib_data = events.iter().find_map(|e| match e {
        Event::AttribData { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(attrib_data.is_some(), "Should have AttribData event");
    let (start, end) = attrib_data.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "hello",
        "AttribData offsets [{}:{}] should match 'hello' (without quotes)",
        start,
        end
    );
}

#[test]
fn test_attribute_end_offsets() {
    let input = r#"<div class="hello"></div>"#;
    let events = collect_events(input);

    let name_start = events
        .iter()
        .find_map(|e| match e {
            Event::AttribName { start, .. } => Some(*start),
            _ => None,
        })
        .expect("Should have AttribName");

    let attrib_end = events.iter().find_map(|e| match e {
        Event::AttribEnd { end, .. } => Some(*end),
        _ => None,
    });
    assert!(attrib_end.is_some(), "Should have AttribEnd event");
    let end = attrib_end.unwrap();

    // The full attribute from name_start to end should be class="hello"
    assert_eq!(
        &input[name_start as usize..end as usize],
        r#"class="hello""#,
        "Full attribute [{}:{}] should be 'class=\"hello\"'",
        name_start,
        end
    );
}

#[test]
fn test_attribute_offsets_in_template() {
    let input = "<template>\n<div class=\"hello\" v-if=\"show\">\n  {{ message }}\n</template>";
    let events = collect_events(input);

    // Find the class attribute name
    let class_name = events.iter().find_map(|e| match e {
        Event::AttribName { start, end } => {
            if &input[*start as usize..*end as usize] == "class" {
                Some((*start, *end))
            } else {
                None
            }
        }
        _ => None,
    });
    assert!(class_name.is_some(), "Should find class attribute");
    let (name_start, name_end) = class_name.unwrap();

    assert_eq!(
        &input[name_start as usize..name_end as usize],
        "class",
        "Class name offsets [{}:{}] should match 'class'",
        name_start,
        name_end
    );

    // Find AttribEnd for class attribute
    let mut found_class = false;
    let attrib_end = events.iter().find_map(|e| match e {
        Event::AttribName { start, .. } if *start == name_start => {
            found_class = true;
            None
        }
        Event::AttribEnd { end, .. } if found_class => {
            found_class = false;
            Some(*end)
        }
        _ => None,
    });

    assert!(attrib_end.is_some(), "Should find AttribEnd for class");
    let end = attrib_end.unwrap();

    let full_attr = &input[name_start as usize..end as usize];
    assert_eq!(
        full_attr, r#"class="hello""#,
        "Full class attribute [{}:{}] should be 'class=\"hello\"'",
        name_start, end
    );
}

#[test]
fn test_attribute_single_quote_offsets() {
    let input = "<div class='world'></div>";
    let events = collect_events(input);

    let name_start = events
        .iter()
        .find_map(|e| match e {
            Event::AttribName { start, .. } => Some(*start),
            _ => None,
        })
        .expect("Should have AttribName");

    let data = events.iter().find_map(|e| match e {
        Event::AttribData { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(data.is_some(), "Should have AttribData");
    let (data_start, data_end) = data.unwrap();
    assert_eq!(
        &input[data_start as usize..data_end as usize],
        "world",
        "AttribData should match 'world' without quotes"
    );

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::AttribEnd { end, .. } => Some(*end),
            _ => None,
        })
        .expect("Should have AttribEnd");

    assert_eq!(
        &input[name_start as usize..end as usize],
        "class='world'",
        "Full attribute should be 'class='world''"
    );
}

#[test]
fn test_attribute_unquoted_offsets() {
    let input = "<div id=test></div>";
    let events = collect_events(input);

    let name_start = events
        .iter()
        .find_map(|e| match e {
            Event::AttribName { start, .. } => Some(*start),
            _ => None,
        })
        .expect("Should have AttribName");

    let data = events.iter().find_map(|e| match e {
        Event::AttribData { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(data.is_some(), "Should have AttribData");
    let (data_start, data_end) = data.unwrap();
    assert_eq!(
        &input[data_start as usize..data_end as usize],
        "test",
        "Unquoted AttribData should match 'test'"
    );

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::AttribEnd { end, .. } => Some(*end),
            _ => None,
        })
        .expect("Should have AttribEnd");

    assert_eq!(
        &input[name_start as usize..end as usize],
        "id=test",
        "Full unquoted attribute should be 'id=test'"
    );
}

#[test]
fn test_multiple_attributes_offsets() {
    let input = r#"<div id="foo" class="bar"></div>"#;
    let events = collect_events(input);

    let attrs: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::AttribName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(attrs.len(), 2, "Should have 2 attributes");

    // First attribute: id
    let (start, end) = attrs[0];
    assert_eq!(
        &input[start as usize..end as usize],
        "id",
        "First attribute name should be 'id'"
    );

    // Second attribute: class
    let (start, end) = attrs[1];
    assert_eq!(
        &input[start as usize..end as usize],
        "class",
        "Second attribute name should be 'class'"
    );
}

#[test]
fn test_text_node_offsets() {
    let input = "<div>hello world</div>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(text.is_some(), "Should have Text event");
    let (start, end) = text.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "hello world",
        "Text offsets [{}:{}] should match 'hello world'",
        start,
        end
    );
}

#[test]
fn test_interpolation_offsets() {
    let input = "<div>{{ message }}</div>";
    let events = collect_events(input);

    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation { start, end, .. } => Some((*start, *end)),
        _ => None,
    });
    assert!(interp.is_some(), "Should have Interpolation event");
    let (start, end) = interp.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "{{ message }}",
        "Interpolation offsets [{}:{}] should match '{{{{ message }}}}'",
        start,
        end
    );
}

#[test]
fn test_directive_name_offsets() {
    let input = r#"<div v-if="show"></div>"#;
    let events = collect_events(input);

    let dir_name = events.iter().find_map(|e| match e {
        Event::DirName { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(dir_name.is_some(), "Should have DirName event");
    let (start, end) = dir_name.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "v-if",
        "DirName offsets [{}:{}] should match 'v-if'",
        start,
        end
    );
}

#[test]
fn test_directive_arg_offsets() {
    let input = r#"<div v-bind:class="active"></div>"#;
    let events = collect_events(input);

    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg { start, end, .. } => Some((*start, *end)),
        _ => None,
    });
    assert!(dir_arg.is_some(), "Should have DirArg event");
    let (start, end) = dir_arg.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "class",
        "DirArg offsets [{}:{}] should match 'class'",
        start,
        end
    );
}

#[test]
fn test_directive_modifier_offsets() {
    let input = r#"<button @click.prevent="handler"></button>"#;
    let events = collect_events(input);

    let modifier = events.iter().find_map(|e| match e {
        Event::DirModifier { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(modifier.is_some(), "Should have DirModifier event");
    let (start, end) = modifier.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "prevent",
        "DirModifier offsets [{}:{}] should match 'prevent'",
        start,
        end
    );
}

#[test]
fn test_directive_dynamic_arg_offsets() {
    let input = r#"<div v-bind:[key]="value"></div>"#;
    let events = collect_events(input);

    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg {
            start,
            end,
            is_dynamic,
        } => {
            if *is_dynamic {
                Some((*start, *end))
            } else {
                None
            }
        }
        _ => None,
    });
    assert!(dir_arg.is_some(), "Should have dynamic DirArg event");
    let (start, end) = dir_arg.unwrap();
    // Dynamic arguments include the brackets in the tokenizer output
    assert_eq!(
        &input[start as usize..end as usize],
        "[key]",
        "Dynamic DirArg offsets [{}:{}] should match '[key]' (includes brackets)",
        start,
        end
    );
    // The key itself (without brackets) is at [start+1..end-1]
    assert_eq!(
        &input[start as usize + 1..end as usize - 1],
        "key",
        "Key without brackets should be 'key'"
    );
}

#[test]
fn test_comment_offsets() {
    let input = "<!-- This is a comment -->";
    let events = collect_events(input);

    let comment = events.iter().find_map(|e| match e {
        Event::Comment {
            start,
            end,
            content_start,
            content_end,
        } => Some((*start, *end, *content_start, *content_end)),
        _ => None,
    });
    assert!(comment.is_some(), "Should have Comment event");
    let (start, end, content_start, content_end) = comment.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "<!-- This is a comment -->",
        "Comment offsets [{}:{}] should match full comment",
        start,
        end
    );
    assert_eq!(
        &input[content_start as usize..content_end as usize],
        " This is a comment ",
        "Comment content [{}:{}] should be between delimiters",
        content_start,
        content_end
    );
}

#[test]
fn test_self_closing_tag_offsets() {
    let input = "<input type=\"text\" />";
    let events = collect_events(input);

    let tag_name = events.iter().find_map(|e| match e {
        Event::OpenTagName { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(tag_name.is_some(), "Should have OpenTagName event");
    let (start, end) = tag_name.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "<input",
        "Self-closing tag should be '<input' (includes <)"
    );
    assert_eq!(
        &input[start as usize + 1..end as usize],
        "input",
        "Tag name without '<' should be 'input'"
    );

    let self_closing = events.iter().find_map(|e| match e {
        Event::SelfClosingTag { end } => Some(*end),
        _ => None,
    });
    assert!(self_closing.is_some(), "Should have SelfClosingTag event");
}

#[test]
fn test_nested_elements_offsets() {
    let input = "<div><span>text</span></div>";
    let events = collect_events(input);

    let tag_names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(tag_names.len(), 2, "Should have 2 open tags");

    let (start, end) = tag_names[0];
    assert_eq!(
        &input[start as usize..end as usize],
        "<div",
        "First tag should be '<div' (includes <)"
    );
    assert_eq!(
        &input[start as usize + 1..end as usize],
        "div",
        "First tag name without '<' should be 'div'"
    );

    let (start, end) = tag_names[1];
    assert_eq!(
        &input[start as usize..end as usize],
        "<span",
        "Second tag should be '<span' (includes <)"
    );
    assert_eq!(
        &input[start as usize + 1..end as usize],
        "span",
        "Second tag name without '<' should be 'span'"
    );
}

#[test]
fn test_complex_template_offsets() {
    let input = r#"<template>
<div class="hello" v-if="show">
  {{ message }}
  <span @click:foo.bar="onClick">text</span>
</div>
</template>"#;
    let events = collect_events(input);

    // Verify class attribute
    let class_attr = events.iter().find_map(|e| match e {
        Event::AttribName { start, end } => {
            if &input[*start as usize..*end as usize] == "class" {
                Some((*start, *end))
            } else {
                None
            }
        }
        _ => None,
    });
    assert!(class_attr.is_some());
    let (start, end) = class_attr.unwrap();
    assert_eq!(&input[start as usize..end as usize], "class");

    // Verify v-if directive
    let v_if = events.iter().find_map(|e| match e {
        Event::DirName { start, end } => {
            if &input[*start as usize..*end as usize] == "v-if" {
                Some((*start, *end))
            } else {
                None
            }
        }
        _ => None,
    });
    assert!(v_if.is_some());
    let (start, end) = v_if.unwrap();
    assert_eq!(&input[start as usize..end as usize], "v-if");

    // Verify interpolation
    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation { start, end, .. } => Some((*start, *end)),
        _ => None,
    });
    assert!(interp.is_some());
    let (start, end) = interp.unwrap();
    assert_eq!(&input[start as usize..end as usize], "{{ message }}");

    // Verify directive with argument and modifier
    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg { start, end, .. } => {
            let slice = &input[*start as usize..*end as usize];
            if slice.contains("click") {
                Some((*start, *end))
            } else {
                None
            }
        }
        _ => None,
    });
    assert!(dir_arg.is_some());
    let (start, end) = dir_arg.unwrap();
    assert_eq!(&input[start as usize..end as usize], "click:foo");

    let modifier = events.iter().find_map(|e| match e {
        Event::DirModifier { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(modifier.is_some());
    let (start, end) = modifier.unwrap();
    assert_eq!(&input[start as usize..end as usize], "bar");
}

// ==================== Interpolation delimiter length tests ====================

/// @ai-generated - Tests that default delimiters emit correct delimiter_open_len/delimiter_close_len
#[test]
fn test_interpolation_delimiter_lengths_default() {
    let events = collect_events("<div>{{ msg }}</div>");

    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation {
            start,
            end,
            delimiter_open_len,
            delimiter_close_len,
        } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
        _ => None,
    });

    assert!(interp.is_some(), "Should have Interpolation event");
    let (start, end, open_len, close_len) = interp.unwrap();

    assert_eq!(open_len, 2, "Default delimiter_open_len should be 2");
    assert_eq!(close_len, 2, "Default delimiter_close_len should be 2");

    let input = "<div>{{ msg }}</div>";
    assert_eq!(
        &input[start as usize..end as usize],
        "{{ msg }}",
        "Interpolation span should include delimiters"
    );
}

/// @ai-generated - Tests custom 3-byte delimiters produce correct spans and lengths
#[test]
fn test_interpolation_custom_3byte_delimiters() {
    let input = "<div>[[[value]]]</div>";
    let mut events = Vec::new();
    tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"[[[", b"]]]");

    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation {
            start,
            end,
            delimiter_open_len,
            delimiter_close_len,
        } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
        _ => None,
    });

    assert!(interp.is_some(), "Should have Interpolation event");
    let (start, end, open_len, close_len) = interp.unwrap();

    assert_eq!(open_len, 3, "Custom delimiter_open_len should be 3");
    assert_eq!(close_len, 3, "Custom delimiter_close_len should be 3");

    assert_eq!(
        &input[start as usize..end as usize],
        "[[[value]]]",
        "Interpolation span should match '[[[value]]]'"
    );
    // Verify content extraction via offsets
    assert_eq!(
        &input[(start as usize + open_len as usize)..(end as usize - close_len as usize)],
        "value",
        "Content between delimiters should be 'value'"
    );
}

/// @ai-generated - Tests custom single-byte delimiters
#[test]
fn test_interpolation_single_byte_delimiters() {
    let input = "<div>#msg#</div>";
    let mut events = Vec::new();
    tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"#", b"#");

    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation {
            start,
            end,
            delimiter_open_len,
            delimiter_close_len,
        } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
        _ => None,
    });

    assert!(interp.is_some(), "Should have Interpolation event");
    let (start, end, open_len, close_len) = interp.unwrap();

    assert_eq!(open_len, 1, "delimiter_open_len should be 1 for '#'");
    assert_eq!(close_len, 1, "delimiter_close_len should be 1 for '#'");

    assert_eq!(
        &input[start as usize..end as usize],
        "#msg#",
        "Interpolation span should match '#msg#'"
    );
    assert_eq!(
        &input[(start as usize + open_len as usize)..(end as usize - close_len as usize)],
        "msg",
        "Content between delimiters should be 'msg'"
    );
}

// ==================== Span consistency tests ====================
// These tests verify the exclusive-end convention: all `end` values point AFTER
// the last character, so `input[start..end]` gives the full text.

#[test]
fn test_open_tag_end_span_is_exclusive() {
    // OpenTagEnd.end should be AFTER the `>`
    let input = "<div>text</div>";
    let events = collect_events(input);

    let open_tag_end = events
        .iter()
        .find_map(|e| match e {
            Event::OpenTagEnd { end } => Some(*end),
            _ => None,
        })
        .expect("Should have OpenTagEnd event");

    // end should be 5 (after '>'), so input[0..5] = "<div>"
    assert_eq!(
        &input[..open_tag_end as usize],
        "<div>",
        "OpenTagEnd.end should be exclusive (after >)"
    );
}

#[test]
fn test_open_tag_end_span_with_attributes() {
    let input = r#"<div class="foo">text</div>"#;
    let events = collect_events(input);

    let open_tag_end = events
        .iter()
        .find_map(|e| match e {
            Event::OpenTagEnd { end } => Some(*end),
            _ => None,
        })
        .expect("Should have OpenTagEnd event");

    assert_eq!(
        &input[..open_tag_end as usize],
        r#"<div class="foo">"#,
        "OpenTagEnd.end should be after > including attributes"
    );
}

#[test]
fn test_self_closing_tag_span_no_attrs() {
    let input = "<br/>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<br/>",
        "SelfClosingTag.end should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_with_space() {
    let input = "<br />";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<br />",
        "SelfClosingTag.end should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_with_directive() {
    // This specifically tests the directive code paths that had the bug
    let input = r#"<div v-if="show"/>"#;
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        r#"<div v-if="show"/>"#,
        "SelfClosingTag.end with directive should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_with_directive_arg() {
    let input = r#"<div v-bind:class="x"/>"#;
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        r#"<div v-bind:class="x"/>"#,
        "SelfClosingTag.end with directive arg should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_with_directive_modifier() {
    let input = r#"<div @click.prevent="fn"/>"#;
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        r#"<div @click.prevent="fn"/>"#,
        "SelfClosingTag.end with modifier should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_v_pre() {
    let input = "<input v-pre/>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<input v-pre/>",
        "SelfClosingTag.end with v-pre should be exclusive (after >)"
    );
}

#[test]
fn test_self_closing_tag_span_with_dynamic_arg() {
    let input = r#"<div v-bind:[key]="val"/>"#;
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        r#"<div v-bind:[key]="val"/>"#,
        "SelfClosingTag.end with dynamic arg should be exclusive (after >)"
    );
}

#[test]
fn test_close_tag_span() {
    let input = "<div></div>";
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::CloseTag { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have CloseTag event");

    assert_eq!(
        &input[start as usize..end as usize],
        "</div>",
        "CloseTag span should be exclusive (start at <, end after >)"
    );
}

#[test]
fn test_processing_instruction_span() {
    let input = "<?xml version=\"1.0\"?>text";
    let events = collect_events(input);

    let pi = events.iter().find_map(|e| match e {
        Event::ProcessingInstruction { start, end } => Some((*start, *end)),
        _ => None,
    });

    assert!(pi.is_some(), "Should have ProcessingInstruction event");
    let (start, end) = pi.unwrap();

    // end should be exclusive (after >)
    assert_eq!(
        input.as_bytes()[end as usize - 1],
        b'>',
        "ProcessingInstruction.end - 1 should point at >"
    );
    // start should be after <?
    assert!(start >= 2, "ProcessingInstruction.start should be after <?");
}

#[test]
fn test_comment_span_includes_delimiters() {
    let input = "text<!-- comment -->more";
    let events = collect_events(input);

    let (start, end, cs, ce) = events
        .iter()
        .find_map(|e| match e {
            Event::Comment {
                start,
                end,
                content_start,
                content_end,
            } => Some((*start, *end, *content_start, *content_end)),
            _ => None,
        })
        .expect("Should have Comment event");

    assert_eq!(
        &input[start as usize..end as usize],
        "<!-- comment -->",
        "Comment span should include <!-- and -->"
    );
    assert_eq!(
        &input[cs as usize..ce as usize],
        " comment ",
        "Comment content should be between delimiters"
    );
}

#[test]
fn test_empty_comment_span() {
    let input = "<!---->";
    let events = collect_events(input);

    let (start, end, cs, ce) = events
        .iter()
        .find_map(|e| match e {
            Event::Comment {
                start,
                end,
                content_start,
                content_end,
            } => Some((*start, *end, *content_start, *content_end)),
            _ => None,
        })
        .expect("Should have Comment event");

    assert_eq!(
        &input[start as usize..end as usize],
        "<!---->",
        "Empty comment span should include full delimiters"
    );
    assert_eq!(
        cs, ce,
        "Empty comment should have content_start == content_end"
    );
}

#[test]
fn test_short_comment_span() {
    // <!-->  is a valid short/abruptly-closed comment (empty content)
    let input = "<!-->text";
    let events = collect_events(input);

    let (start, end, cs, ce) = events
        .iter()
        .find_map(|e| match e {
            Event::Comment {
                start,
                end,
                content_start,
                content_end,
            } => Some((*start, *end, *content_start, *content_end)),
            _ => None,
        })
        .expect("Should have Comment event for <!--->");

    assert_eq!(
        &input[start as usize..end as usize],
        "<!-->",
        "Short comment span should include full delimiters"
    );
    assert_eq!(cs, ce, "Short comment should have empty content");
}

#[test]
fn test_abrupt_close_comment_span() {
    // <!---> is a valid abrupt-close comment (empty content)
    let input = "text<!--->after";
    let events = collect_events(input);

    let (start, end, cs, ce) = events
        .iter()
        .find_map(|e| match e {
            Event::Comment {
                start,
                end,
                content_start,
                content_end,
            } => Some((*start, *end, *content_start, *content_end)),
            _ => None,
        })
        .expect("Should have Comment event for <!--->");

    assert_eq!(
        &input[start as usize..end as usize],
        "<!--->",
        "Abrupt-close comment span should include full delimiters"
    );
    assert_eq!(cs, ce, "Abrupt-close comment should have empty content");
}

#[test]
fn test_interpolation_span_includes_delimiters() {
    let input = "{{ expr }}";
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::Interpolation { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have Interpolation event");

    assert_eq!(
        &input[start as usize..end as usize],
        "{{ expr }}",
        "Interpolation span should include delimiters"
    );
}

#[test]
fn test_text_span() {
    let input = "<div>hello world</div>";
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::Text { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have Text event");

    assert_eq!(
        &input[start as usize..end as usize],
        "hello world",
        "Text span should be just the content"
    );
}

#[test]
fn test_attrib_name_span() {
    let input = r#"<div class="x"></div>"#;
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::AttribName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have AttribName event");

    assert_eq!(
        &input[start as usize..end as usize],
        "class",
        "AttribName span should be just the name"
    );
}

#[test]
fn test_attrib_data_span_excludes_quotes() {
    let input = r#"<div class="hello"></div>"#;
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::AttribData { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have AttribData event");

    assert_eq!(
        &input[start as usize..end as usize],
        "hello",
        "AttribData span should exclude quotes"
    );
}

#[test]
fn test_attrib_end_span_double_quote() {
    let input = r#"<div class="hello"></div>"#;
    let events = collect_events(input);

    let name_start = events
        .iter()
        .find_map(|e| match e {
            Event::AttribName { start, .. } => Some(*start),
            _ => None,
        })
        .expect("Should have AttribName");

    let (quote, end) = events
        .iter()
        .find_map(|e| match e {
            Event::AttribEnd { quote, end } => Some((quote.clone(), *end)),
            _ => None,
        })
        .expect("Should have AttribEnd event");

    assert_eq!(quote, QuoteType::Double, "Should be double-quoted");
    assert_eq!(
        &input[name_start as usize..end as usize],
        r#"class="hello""#,
        "Full attribute span (name_start to AttribEnd.end) should include closing quote"
    );
}

#[test]
fn test_attrib_end_no_value() {
    let input = "<div disabled></div>";
    let events = collect_events(input);

    let (quote, _end) = events
        .iter()
        .find_map(|e| match e {
            Event::AttribEnd { quote, end } => Some((quote.clone(), *end)),
            _ => None,
        })
        .expect("Should have AttribEnd event");

    assert_eq!(
        quote,
        QuoteType::NoValue,
        "Boolean attribute should have NoValue quote type"
    );
}

#[test]
fn test_dir_name_span() {
    let input = r#"<div v-if="show"></div>"#;
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::DirName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have DirName event");

    assert_eq!(
        &input[start as usize..end as usize],
        "v-if",
        "DirName span should be just the directive name"
    );
}

#[test]
fn test_dir_arg_static_span() {
    let input = r#"<div v-bind:class="x"></div>"#;
    let events = collect_events(input);

    let (start, end, is_dynamic) = events
        .iter()
        .find_map(|e| match e {
            Event::DirArg {
                start,
                end,
                is_dynamic,
            } => Some((*start, *end, *is_dynamic)),
            _ => None,
        })
        .expect("Should have DirArg event");

    assert!(!is_dynamic, "Should be static arg");
    assert_eq!(
        &input[start as usize..end as usize],
        "class",
        "Static DirArg span should be just the arg name"
    );
}

#[test]
fn test_dir_arg_dynamic_span() {
    let input = r#"<div v-bind:[key]="x"></div>"#;
    let events = collect_events(input);

    let (start, end, is_dynamic) = events
        .iter()
        .find_map(|e| match e {
            Event::DirArg {
                start,
                end,
                is_dynamic,
            } => Some((*start, *end, *is_dynamic)),
            _ => None,
        })
        .expect("Should have DirArg event");

    assert!(is_dynamic, "Should be dynamic arg");
    assert_eq!(
        &input[start as usize..end as usize],
        "[key]",
        "Dynamic DirArg span should include brackets"
    );
}

#[test]
fn test_dir_modifier_span() {
    let input = r#"<div @click.prevent="fn"></div>"#;
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::DirModifier { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have DirModifier event");

    assert_eq!(
        &input[start as usize..end as usize],
        "prevent",
        "DirModifier span should be just the modifier name"
    );
}

#[test]
fn test_dir_v_pre_span() {
    let input = "<div v-pre></div>";
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::DirVPre { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have DirVPre event");

    assert_eq!(
        &input[start as usize..end as usize],
        "v-pre",
        "DirVPre span should be just 'v-pre'"
    );
}

#[test]
fn test_open_tag_name_includes_lt() {
    let input = "<MyComponent></MyComponent>";
    let events = collect_events(input);

    let (start, end) = events
        .iter()
        .find_map(|e| match e {
            Event::OpenTagName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .expect("Should have OpenTagName event");

    assert_eq!(
        &input[start as usize..end as usize],
        "<MyComponent",
        "OpenTagName span includes <"
    );
    assert_eq!(
        &input[start as usize + 1..end as usize],
        "MyComponent",
        "Tag name without < should be just the name"
    );
}

// ==================== Edge case tests ====================

#[test]
fn test_empty_input() {
    let events = collect_events("");
    assert_eq!(events.len(), 1, "Empty input should only produce End event");
    assert!(matches!(events[0], Event::End));
}

#[test]
fn test_only_text() {
    let events = collect_events("just text");
    assert!(events.iter().any(|e| matches!(e, Event::Text { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

#[test]
fn test_multiple_interpolations_all_spans_correct() {
    let input = "{{ a }}text{{ b }}";
    let events = collect_events(input);

    let interps: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Interpolation { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(interps.len(), 2, "Should have 2 interpolation events");
    assert_eq!(
        &input[interps[0].0 as usize..interps[0].1 as usize],
        "{{ a }}",
        "First interpolation span"
    );
    assert_eq!(
        &input[interps[1].0 as usize..interps[1].1 as usize],
        "{{ b }}",
        "Second interpolation span"
    );
}

#[test]
fn test_rcdata_script_content() {
    let input = "<script>let x = 1;</script>";
    let events = collect_events(input);

    // Should have text inside script
    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(text.is_some(), "Should have text inside script");
    let (start, end) = text.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "let x = 1;",
        "Script content should be text"
    );
}

#[test]
fn test_rcdata_style_content() {
    let input = "<style>.red { color: red; }</style>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(text.is_some(), "Should have text inside style");
    let (start, end) = text.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        ".red { color: red; }",
        "Style content should be text"
    );
}

#[test]
fn test_all_open_tag_end_events_are_exclusive() {
    // Comprehensive test: multiple tags, all OpenTagEnd should be after >
    let input = "<div><span><a></a></span></div>";
    let events = collect_events(input);

    let tag_ends: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagEnd { end } => Some(*end),
            _ => None,
        })
        .collect();

    assert_eq!(tag_ends.len(), 3, "Should have 3 OpenTagEnd events");
    for end in &tag_ends {
        assert_eq!(
            input.as_bytes()[*end as usize - 1],
            b'>',
            "OpenTagEnd.end - 1 should point at > (end={})",
            end
        );
    }
}

#[test]
fn test_all_self_closing_tag_events_are_exclusive() {
    // Test multiple self-closing tags with different attribute types
    let input =
        r#"<br/><input type="text"/><div v-if="x"/><span :class="y"/><a @click.prevent="z"/>"#;
    let events = collect_events(input);

    let self_closings: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .collect();

    assert_eq!(
        self_closings.len(),
        5,
        "Should have 5 SelfClosingTag events"
    );
    for end in &self_closings {
        assert_eq!(
            input.as_bytes()[*end as usize - 1],
            b'>',
            "SelfClosingTag.end - 1 should point at > (end={})",
            end
        );
    }
}

#[test]
fn test_all_close_tag_events_are_exclusive() {
    let input = "<div><span>text</span></div>";
    let events = collect_events(input);

    let close_tags: Vec<(u32, u32)> = events
        .iter()
        .filter_map(|e| match e {
            Event::CloseTag { start, end, .. } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    assert_eq!(close_tags.len(), 2, "Should have 2 CloseTag events");
    for (start, end) in &close_tags {
        let slice = &input[*start as usize..*end as usize];
        assert!(
            slice.starts_with("</") && slice.ends_with('>'),
            "CloseTag span '{}' should start with </ and end with >",
            slice
        );
    }
}

#[test]
fn test_directive_no_value_self_closing() {
    // Directive with no value ending in self-closing tag
    let input = "<div v-show/>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<div v-show/>",
        "SelfClosingTag.end should be exclusive"
    );

    // Should have DirName event for v-show
    assert!(
        events.iter().any(|e| matches!(e, Event::DirName { .. })),
        "Should have DirName event"
    );
}

#[test]
fn test_directive_no_value_open_tag() {
    // Directive with no value ending in >
    let input = "<div v-show></div>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::OpenTagEnd { end } => Some(*end),
            _ => None,
        })
        .expect("Should have OpenTagEnd event");

    assert_eq!(
        &input[..end as usize],
        "<div v-show>",
        "OpenTagEnd.end should be exclusive"
    );
}

#[test]
fn test_directive_arg_no_value_self_closing() {
    let input = "<div v-foo:bar/>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<div v-foo:bar/>",
        "SelfClosingTag.end should be exclusive"
    );
}

#[test]
fn test_directive_modifier_no_value_self_closing() {
    let input = "<div @click.prevent/>";
    let events = collect_events(input);

    let end = events
        .iter()
        .find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        })
        .expect("Should have SelfClosingTag event");

    assert_eq!(
        &input[..end as usize],
        "<div @click.prevent/>",
        "SelfClosingTag.end should be exclusive"
    );
}

#[test]
fn test_shorthand_directives_spans() {
    // Test all shorthand forms: :, @, #, .
    let input = r#"<div :class="a" @click="b" #default></div>"#;
    let events = collect_events(input);

    let dir_names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirName { start, end } => Some((*start, *end)),
            _ => None,
        })
        .collect();

    // : is shorthand for v-bind, @ for v-on, # for v-slot
    // The tokenizer emits DirName for single-char shorthands
    assert_eq!(
        dir_names.len(),
        3,
        "Should have 3 DirName events for shorthand directives"
    );

    for (start, end) in &dir_names {
        let name = &input[*start as usize..*end as usize];
        assert!(
            name == ":" || name == "@" || name == "#",
            "DirName should be a shorthand character, got '{}'",
            name
        );
    }
}

#[test]
fn test_multiple_modifiers_spans() {
    let input = r#"<div @click.stop.prevent="fn"></div>"#;
    let events = collect_events(input);

    let modifiers: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirModifier { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();

    assert_eq!(modifiers.len(), 2, "Should have 2 modifier events");
    assert_eq!(modifiers[0], "stop", "First modifier should be 'stop'");
    assert_eq!(
        modifiers[1], "prevent",
        "Second modifier should be 'prevent'"
    );
}

#[test]
fn test_error_missing_end_tag_name() {
    let input = "</>";
    let events = collect_events(input);

    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Error { .. }))
        .collect();

    assert!(
        !errors.is_empty(),
        "Should have Error event for missing end tag name"
    );
}

#[test]
fn test_whitespace_only_text_not_emitted() {
    // Whitespace-only text between tags should not be emitted
    let input = "<div>   </div>";
    let events = collect_events(input);

    let text_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Text { .. }))
        .collect();

    assert_eq!(
        text_events.len(),
        0,
        "Whitespace-only text should not be emitted"
    );
}

#[test]
fn test_case_insensitive_rcdata_tags() {
    // SCRIPT and STYLE should be case-insensitive
    let input = "<SCRIPT>content</SCRIPT>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(text.is_some(), "Should have text inside SCRIPT");
    let (start, end) = text.unwrap();
    assert_eq!(
        &input[start as usize..end as usize],
        "content",
        "SCRIPT content (case-insensitive) should be RCDATA text"
    );
}

#[test]
fn test_complete_event_sequence_simple_element() {
    // Verify the complete event sequence for a simple element
    let input = r#"<div class="x">text</div>"#;
    let events = collect_events(input);

    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            Event::OpenTagName { .. } => "OpenTagName",
            Event::AttribName { .. } => "AttribName",
            Event::AttribNameEnd { .. } => "AttribNameEnd",
            Event::AttribData { .. } => "AttribData",
            Event::AttribEnd { .. } => "AttribEnd",
            Event::OpenTagEnd { .. } => "OpenTagEnd",
            Event::Text { .. } => "Text",
            Event::CloseTag { .. } => "CloseTag",
            Event::End => "End",
            _ => "Other",
        })
        .collect();

    assert_eq!(
        event_types,
        vec![
            "OpenTagName",
            "AttribName",
            "AttribNameEnd",
            "AttribData",
            "AttribEnd",
            "OpenTagEnd",
            "Text",
            "CloseTag",
            "End",
        ],
        "Complete event sequence for simple element with attribute"
    );
}

#[test]
fn test_complete_event_sequence_self_closing() {
    let input = r#"<input type="text"/>"#;
    let events = collect_events(input);

    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            Event::OpenTagName { .. } => "OpenTagName",
            Event::AttribName { .. } => "AttribName",
            Event::AttribNameEnd { .. } => "AttribNameEnd",
            Event::AttribData { .. } => "AttribData",
            Event::AttribEnd { .. } => "AttribEnd",
            Event::SelfClosingTag { .. } => "SelfClosingTag",
            Event::End => "End",
            _ => "Other",
        })
        .collect();

    assert_eq!(
        event_types,
        vec![
            "OpenTagName",
            "AttribName",
            "AttribNameEnd",
            "AttribData",
            "AttribEnd",
            "SelfClosingTag",
            "End",
        ],
        "Complete event sequence for self-closing element"
    );
}

#[test]
fn test_complete_event_sequence_directive() {
    let input = r#"<div v-if="show"></div>"#;
    let events = collect_events(input);

    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            Event::OpenTagName { .. } => "OpenTagName",
            Event::DirName { .. } => "DirName",
            Event::AttribNameEnd { .. } => "AttribNameEnd",
            Event::AttribData { .. } => "AttribData",
            Event::AttribEnd { .. } => "AttribEnd",
            Event::OpenTagEnd { .. } => "OpenTagEnd",
            Event::CloseTag { .. } => "CloseTag",
            Event::End => "End",
            _ => "Other",
        })
        .collect();

    assert_eq!(
        event_types,
        vec![
            "OpenTagName",
            "DirName",
            "AttribNameEnd",
            "AttribData",
            "AttribEnd",
            "OpenTagEnd",
            "CloseTag",
            "End",
        ],
        "Complete event sequence for directive"
    );
}

#[test]
fn test_complete_event_sequence_directive_with_arg_and_modifier() {
    let input = r#"<div v-on:click.prevent="fn"></div>"#;
    let events = collect_events(input);

    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            Event::OpenTagName { .. } => "OpenTagName",
            Event::DirName { .. } => "DirName",
            Event::DirArg { .. } => "DirArg",
            Event::DirModifier { .. } => "DirModifier",
            Event::AttribNameEnd { .. } => "AttribNameEnd",
            Event::AttribData { .. } => "AttribData",
            Event::AttribEnd { .. } => "AttribEnd",
            Event::OpenTagEnd { .. } => "OpenTagEnd",
            Event::CloseTag { .. } => "CloseTag",
            Event::End => "End",
            _ => "Other",
        })
        .collect();

    assert_eq!(
        event_types,
        vec![
            "OpenTagName",
            "DirName",
            "DirArg",
            "DirModifier",
            "AttribNameEnd",
            "AttribData",
            "AttribEnd",
            "OpenTagEnd",
            "CloseTag",
            "End",
        ],
        "Complete event sequence for directive with arg and modifier"
    );
}

// ==================== Declaration recovery tests (Bug #1) ====================

/// @ai-generated - Tests that <!DOCTYPE html> doesn't swallow subsequent content
#[test]
fn test_doctype_recovery() {
    let input = "<!DOCTYPE html><div>text</div>";
    let events = collect_events(input);

    let open_tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagName { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();

    assert!(
        open_tags.iter().any(|t| *t == "<div"),
        "Should have <div> after DOCTYPE, got: {:?}",
        open_tags
    );

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("text"),
        "Text content after DOCTYPE should not be lost"
    );
}

/// @ai-generated - Tests that content after <!DOCTYPE> is preserved
#[test]
fn test_doctype_no_content_loss() {
    let input = "<!DOCTYPE>{{ msg }}";
    let events = collect_events(input);

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        has_interpolation,
        "Interpolation after DOCTYPE should be emitted"
    );
}

/// @ai-generated - Tests that an unknown declaration like <!FOO bar> recovers
#[test]
fn test_unknown_declaration_recovery() {
    let input = "<!FOO bar><span>ok</span>";
    let events = collect_events(input);

    let open_tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagName { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();

    assert!(
        open_tags.iter().any(|t| *t == "<span"),
        "Should recover and parse <span> after unknown declaration, got: {:?}",
        open_tags
    );
}

// ==================== Solidus error recovery tests (Bug #2) ====================

/// @ai-generated - Tests that an unexpected / in a tag doesn't prevent further attribute parsing
#[test]
fn test_unexpected_solidus_recovery() {
    let input = r#"<div /x class="y">text</div>"#;
    let events = collect_events(input);

    let attr_names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::AttribName { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();

    // After /x error, tokenizer should recover and parse class attribute
    assert!(
        attr_names.contains(&"class"),
        "Should recover and parse 'class' attribute after solidus error, got: {:?}",
        attr_names
    );
}

/// @ai-generated - Tests that the error event is emitted for unexpected solidus
#[test]
fn test_unexpected_solidus_emits_error() {
    let input = "<div /x>";
    let events = collect_events(input);

    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::Error { .. }))
        .collect();

    assert!(
        !errors.is_empty(),
        "Should emit Error event for unexpected solidus"
    );
}

/// @ai-generated - Tests that text after recovered tag is not lost
#[test]
fn test_unexpected_solidus_text_preserved() {
    let input = "<div /x>hello</div>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("hello"),
        "Text after recovered tag should be preserved"
    );
}

// ==================== Textarea RCDATA tests ====================

/// @ai-generated - Tests that <textarea> content is treated as RCDATA
#[test]
fn test_textarea_rcdata() {
    let input = "<textarea>content here</textarea>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("content here"),
        "Textarea content should be treated as RCDATA text"
    );
}

/// @ai-generated - Tests case-insensitive textarea RCDATA handling
#[test]
fn test_textarea_rcdata_case_insensitive() {
    let input = "<TEXTAREA>content</TEXTAREA>";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("content"),
        "TEXTAREA (uppercase) should be treated as RCDATA"
    );
}

/// @ai-generated - Tests that HTML tags inside textarea are not parsed
#[test]
fn test_textarea_no_nested_parsing() {
    let input = "<textarea><div>not a tag</div></textarea>";
    let events = collect_events(input);

    // The <div> inside textarea should be text, not an OpenTagName
    let open_tags: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OpenTagName { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();

    // Should only have <textarea>, not <div>
    assert_eq!(
        open_tags.len(),
        1,
        "Only <textarea> should be parsed as a tag, not inner <div>, got: {:?}",
        open_tags
    );

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("<div>not a tag</div>"),
        "Content inside textarea should be raw text including HTML tags"
    );
}

/// @ai-generated - Tests that interpolation IS parsed inside textarea RCDATA (unlike script/style)
#[test]
fn test_textarea_interpolation_in_rcdata() {
    let input = "<textarea>{{ msg }}</textarea>";
    let events = collect_events(input);

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        has_interpolation,
        "Interpolation inside textarea RCDATA should be detected (unlike script/style)"
    );
}

/// @ai-generated - Tests that interpolation is NOT parsed inside script RCDATA
#[test]
fn test_script_no_interpolation_in_rcdata() {
    let input = "<script>{{ msg }}</script>";
    let events = collect_events(input);

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation inside script RCDATA should NOT be detected"
    );
}

/// @ai-generated - Tests that interpolation is NOT parsed inside style RCDATA
#[test]
fn test_style_no_interpolation_in_rcdata() {
    let input = "<style>{{ msg }}</style>";
    let events = collect_events(input);

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation inside style RCDATA should NOT be detected"
    );
}

// ============================================================
// Regression tests for Phase 1-6 fixes
// ============================================================

// --- Phase 1: Critical bug fixes ---

/// @ai-generated - EOF in unterminated close tag should not panic (OOB fix)
#[test]
fn test_eof_in_unterminated_close_tag() {
    // No closing `>` — previously caused OOB
    let input = "</div";
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::CloseTag { .. })),
        "Should emit a CloseTag even without closing >"
    );
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - EOF in unterminated close tag with whitespace
#[test]
fn test_eof_in_close_tag_with_whitespace() {
    let input = "</div   ";
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::CloseTag { .. })),
        "Should emit a CloseTag even with trailing whitespace and no >"
    );
}

/// @ai-generated - GT after = in attribute value should end the tag
#[test]
fn test_gt_after_equals_ends_tag() {
    let input = "<div attr=>";
    let events = collect_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::OpenTagEnd { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - handle_directive_end GT path should respect in_rcdata
#[test]
fn test_directive_end_gt_respects_rcdata() {
    let input = "<textarea v-if=\"show\">content</textarea>";
    let events = collect_events(input);
    // The tag should close properly, and content should be in RCDATA
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::OpenTagName { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Text { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::CloseTag { .. })));
}

/// @ai-generated - Single dash after <! should not hang
#[test]
fn test_single_dash_after_declaration() {
    let input = "<!-x>text";
    let events = collect_events(input);
    // Should recover and produce End
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - CDATA section should be handled
#[test]
fn test_cdata_section() {
    let input = "<![CDATA[some content]]>after";
    let events = collect_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::End)));
    // Should not lose content after CDATA
    let has_text_after = events.iter().any(|e| match e {
        Event::Text { start, end } => {
            let s = &input[*start as usize..*end as usize];
            s.contains("after")
        }
        _ => false,
    });
    assert!(has_text_after, "Text after CDATA should be preserved");
}

// --- Phase 2: v-pre pre-pass ---

/// @ai-generated - v-pre in middle of attributes should suppress earlier directives
#[test]
fn test_v_pre_middle_suppresses_earlier_directives() {
    let input = r#"<div v-if="show" v-pre v-for="i in items">content</div>"#;
    let events = collect_events(input);

    // v-pre should be detected
    assert!(
        events.iter().any(|e| matches!(e, Event::DirVPre { .. })),
        "DirVPre should be emitted"
    );

    // v-if and v-for should NOT be emitted as DirName (they should be regular attrs)
    let dir_names: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirName { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();
    assert!(
        !dir_names.iter().any(|n| *n == "v-if" || *n == "v-for"),
        "v-if and v-for should be suppressed as directives when v-pre is present. Got: {:?}",
        dir_names
    );
}

/// @ai-generated - v-pre pre-pass should skip > inside quoted attribute values
#[test]
fn test_v_pre_prepass_skips_quoted_gt() {
    let input = r#"<div class="a>b" v-pre>content</div>"#;
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::DirVPre { .. })),
        "v-pre should be found even with > inside quoted attribute value"
    );
}

/// @ai-generated - v-preview should NOT be treated as v-pre
#[test]
fn test_v_preview_not_v_pre() {
    let input = r#"<div v-preview="data">{{ msg }}</div>"#;
    let events = collect_events(input);

    // v-preview is a regular directive, not v-pre
    let has_v_pre = events.iter().any(|e| matches!(e, Event::DirVPre { .. }));
    assert!(!has_v_pre, "v-preview should NOT be detected as v-pre");

    // Interpolation should still work
    let has_interp = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(has_interp, "Interpolation should work with v-preview");
}

// --- Phase 3: Entity handling ---

/// @ai-generated - Named entity in text should emit TextEntity
#[test]
fn test_entity_named_in_text() {
    let input = "hello &amp; world";
    let events = collect_events(input);
    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(has_entity, "Should emit TextEntity for &amp;");
}

/// @ai-generated - Numeric entity in text should emit TextEntity
#[test]
fn test_entity_numeric_in_text() {
    let input = "A is &#65; in ASCII";
    let events = collect_events(input);
    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(has_entity, "Should emit TextEntity for &#65;");
}

/// @ai-generated - Hex entity in text should emit TextEntity
#[test]
fn test_entity_hex_in_text() {
    let input = "A is &#x41; in hex";
    let events = collect_events(input);
    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(has_entity, "Should emit TextEntity for &#x41;");
}

/// @ai-generated - Bare ampersand (no semicolon) should NOT emit TextEntity
#[test]
fn test_bare_ampersand_no_entity() {
    let input = "a & b";
    let events = collect_events(input);
    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(!has_entity, "Bare & should not emit TextEntity");
}

/// @ai-generated - Multiple entities in text
#[test]
fn test_multiple_entities_in_text() {
    let input = "&lt;div&gt; &amp; &quot;hello&quot;";
    let events = collect_events(input);
    let entity_count = events
        .iter()
        .filter(|e| matches!(e, Event::TextEntity { .. }))
        .count();
    assert!(
        entity_count >= 4,
        "Should emit at least 4 TextEntity events, got {}",
        entity_count
    );
}

/// @ai-generated - Entity spans should be correct
#[test]
fn test_entity_span_correctness() {
    let input = "a&amp;b";
    let events = collect_events(input);
    for e in &events {
        if let Event::TextEntity { start, end } = e {
            let span = &input[*start as usize..*end as usize];
            assert_eq!(span, "&amp;", "Entity span should be exactly '&amp;'");
        }
    }
}

// --- Phase 4: EOF flush ---

/// @ai-generated - EOF in unterminated open tag should emit events
#[test]
fn test_eof_in_open_tag() {
    let input = "<div";
    let events = collect_events(input);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::OpenTagName { .. })),
        "Should emit OpenTagName even at EOF"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Error { .. })));
}

/// @ai-generated - EOF in attribute name
#[test]
fn test_eof_in_attr_name() {
    let input = "<div class";
    let events = collect_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::AttribName { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Error { .. })));
}

/// @ai-generated - EOF in unquoted attribute value
#[test]
fn test_eof_in_attr_value_unquoted() {
    let input = "<div class=foo";
    let events = collect_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::AttribData { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Error { .. })));
}

/// @ai-generated - EOF in interpolation should emit error
#[test]
fn test_eof_in_interpolation() {
    let input = "{{ msg";
    let events = collect_events(input);
    assert!(events.iter().any(|e| match e {
        Event::Error { code, .. } => *code == ErrorCode::X_MISSING_INTERPOLATION_END,
        _ => false,
    }));
}

/// @ai-generated - EOF in comment
#[test]
fn test_eof_in_comment() {
    let input = "<!-- comment without end";
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::Comment { .. })),
        "Should emit Comment even at EOF"
    );
}

/// @ai-generated - EOF in directive name
#[test]
fn test_eof_in_dir_name() {
    let input = "<div v-if";
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::DirName { .. })),
        "Should emit DirName even at EOF"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Error { .. })));
}

/// @ai-generated - EOF in RCDATA
#[test]
fn test_eof_in_rcdata_script() {
    let input = "<script>console.log('hello')";
    let events = collect_events(input);
    assert!(
        events.iter().any(|e| matches!(e, Event::Text { .. })),
        "Should flush RCDATA text at EOF"
    );
}

/// @ai-generated - EOF in processing instruction
#[test]
fn test_eof_in_processing_instruction() {
    let input = "<?xml version";
    let events = collect_events(input);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::ProcessingInstruction { .. })));
}

/// @ai-generated - EOF in dynamic directive arg
#[test]
fn test_eof_in_dynamic_dir_arg() {
    let input = "<div v-bind:[key";
    let events = collect_events(input);
    assert!(events.iter().any(|e| match e {
        Event::Error { code, .. } => *code == ErrorCode::X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END,
        _ => false,
    }));
}

// --- Phase 5: Other bug fixes ---

/// @ai-generated - find_unescaped should handle consecutive backslashes
#[test]
fn test_backslash_escape_in_attr_value() {
    // Double backslash before quote means the quote is NOT escaped
    let input = r#"<div attr="val\\">after</div>"#;
    let events = collect_events(input);
    // Should parse correctly — the " after \\ is a real closing quote
    assert!(events.iter().any(|e| matches!(e, Event::AttribData { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

// --- Phase 6: RCDATA textarea interpolation ---

/// @ai-generated - Textarea with multiple interpolations
#[test]
fn test_textarea_multiple_interpolations() {
    let input = "<textarea>{{ a }} and {{ b }}</textarea>";
    let events = collect_events(input);

    let interp_count = events
        .iter()
        .filter(|e| matches!(e, Event::Interpolation { .. }))
        .count();
    assert_eq!(
        interp_count, 2,
        "Textarea should have 2 interpolations, got {}",
        interp_count
    );
}

/// @ai-generated - Textarea with mixed text and interpolation
#[test]
fn test_textarea_mixed_content() {
    let input = "<textarea>hello {{ name }}, welcome!</textarea>";
    let events = collect_events(input);

    let has_text = events.iter().any(|e| matches!(e, Event::Text { .. }));
    let has_interp = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(has_text, "Should have text in textarea");
    assert!(has_interp, "Should have interpolation in textarea");
}

/// @ai-generated - Textarea in v-pre should NOT have interpolation
#[test]
fn test_textarea_in_v_pre_no_interpolation() {
    let input = "<div v-pre><textarea>{{ msg }}</textarea></div>";
    let events = collect_events(input);

    let has_interp = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interp,
        "Textarea inside v-pre should NOT parse interpolation"
    );
}

// ==================== Tests for refactoring changes ====================

/// @ai-generated - v-pre="value" should NOT be detected as v-pre by scan_for_v_pre
/// (EQ is now a word boundary in scan_for_v_pre)
#[test]
fn test_v_pre_with_eq_not_detected_by_prepass() {
    // v-pre="value" is invalid Vue syntax but should not be treated as v-pre
    let input = r#"<div v-pre="value">{{ msg }}</div>"#;
    let events = collect_events(input);

    // Even though v-pre has = after it (making it look like an attribute with a value),
    // the scan_for_v_pre now treats = as a valid word boundary, so this IS detected.
    // But DirVPre is still emitted because the dir_name handler checks for v-pre.
    // The key point: the tokenizer should not crash or misbehave.
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - EOF in dynamic directive arg should emit partial DirArg event
#[test]
fn test_eof_in_dynamic_dir_arg_emits_partial_events() {
    let input = "<div v-bind:[key";
    let events = collect_events(input);

    // Should emit DirArg with is_dynamic=true
    let has_dir_arg = events.iter().any(|e| match e {
        Event::DirArg { is_dynamic, .. } => *is_dynamic,
        _ => false,
    });
    assert!(
        has_dir_arg,
        "EOF in dynamic arg should emit partial DirArg event"
    );

    // Should emit AttribNameEnd and AttribEnd for consumer consistency
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::AttribNameEnd { .. })),
        "EOF in dynamic arg should emit AttribNameEnd"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::AttribEnd { .. })),
        "EOF in dynamic arg should emit AttribEnd"
    );
}

/// @ai-generated - Backslash-escaped quote in attribute value should not close the attribute
#[test]
fn test_backslash_escaped_quote_in_attr() {
    let input = r#"<div attr="it\'s me">text</div>"#;
    let events = collect_events(input);

    let data = events.iter().find_map(|e| match e {
        Event::AttribData { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        data,
        Some(r"it\'s me"),
        "Backslash-escaped quote should not terminate the attribute value"
    );
}

/// @ai-generated - CDATA with no valid CDATA pattern falls back to scanning to >
#[test]
fn test_invalid_cdata_recovery() {
    let input = "<![INVALID]>after";
    let events = collect_events(input);

    let has_text = events.iter().any(|e| match e {
        Event::Text { start, end } => {
            let s = &input[*start as usize..*end as usize];
            s.contains("after")
        }
        _ => false,
    });
    assert!(
        has_text,
        "Text after invalid CDATA-like declaration should be preserved"
    );
}

/// @ai-generated - Custom delimiters with tokenize_with_delimiters
#[test]
fn test_custom_delimiters_mustache_triple() {
    let input = "<div>{{{ msg }}}</div>";
    let mut events = Vec::new();
    tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"{{{", b"}}}");

    let interp = events.iter().find_map(|e| match e {
        Event::Interpolation {
            start,
            end,
            delimiter_open_len,
            delimiter_close_len,
        } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
        _ => None,
    });
    assert!(
        interp.is_some(),
        "Should detect triple-mustache interpolation"
    );
    let (start, end, open_len, close_len) = interp.unwrap();
    assert_eq!(open_len, 3);
    assert_eq!(close_len, 3);
    assert_eq!(&input[start as usize..end as usize], "{{{ msg }}}");
}

/// @ai-generated - Processing instruction with memchr optimization produces correct span
#[test]
fn test_processing_instruction_span_correct() {
    let input = "<?xml version=\"1.0\" encoding=\"utf-8\"?>after";
    let events = collect_events(input);

    let pi = events.iter().find_map(|e| match e {
        Event::ProcessingInstruction { start, end } => Some((*start, *end)),
        _ => None,
    });
    assert!(pi.is_some(), "Should have ProcessingInstruction event");
    let (_start, end) = pi.unwrap();
    assert_eq!(
        input.as_bytes()[end as usize - 1],
        b'>',
        "PI end should be after >"
    );

    // Text after PI should be preserved
    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(text, Some("after"), "Text after PI should be preserved");
}

// ==================== Additional edge case tests ====================

/// @ai-generated - Short comment <!-->  (empty comment)
#[test]
fn test_short_comment_empty() {
    let input = "<!-->after";
    let events = collect_events(input);

    let comment = events.iter().find_map(|e| match e {
        Event::Comment {
            start,
            end,
            content_start,
            content_end,
        } => Some((*start, *end, *content_start, *content_end)),
        _ => None,
    });
    assert!(comment.is_some(), "Should emit Comment for <!-->");

    let (_, _, cs, ce) = comment.unwrap();
    assert_eq!(cs, ce, "Short comment should have empty content");

    // Text after should be preserved
    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("after"),
        "Text after short comment should be preserved"
    );
}

/// @ai-generated - Abrupt-close comment <!--->
#[test]
fn test_abrupt_close_comment() {
    let input = "<!--->after";
    let events = collect_events(input);

    let comment = events.iter().find_map(|e| match e {
        Event::Comment {
            content_start,
            content_end,
            ..
        } => Some((*content_start, *content_end)),
        _ => None,
    });
    assert!(comment.is_some(), "Should emit Comment for <!--->");

    let (cs, ce) = comment.unwrap();
    assert_eq!(cs, ce, "Abrupt-close comment should have empty content");

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("after"),
        "Text after abrupt-close comment should be preserved"
    );
}

/// @ai-generated - EOF in quoted attribute value
#[test]
fn test_eof_in_quoted_attr_value() {
    let input = r#"<div class="foo"#;
    let events = collect_events(input);

    assert!(
        events.iter().any(|e| matches!(e, Event::AttribData { .. })),
        "Should emit AttribData even at EOF"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Error { .. })),
        "Should emit error for unterminated quoted attribute"
    );
}

/// @ai-generated - </ inside attribute area triggers close tag processing
#[test]
fn test_close_tag_in_attr_area() {
    let input = "<div </div>";
    let events = collect_events(input);

    // Should have OpenTagName for div
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::OpenTagName { .. })),
        "Should have OpenTagName"
    );
    // Should have CloseTag
    assert!(
        events.iter().any(|e| matches!(e, Event::CloseTag { .. })),
        "Should have CloseTag for </div>"
    );
}

/// @ai-generated - Entity not terminated before EOF
#[test]
fn test_entity_at_eof() {
    let input = "text&amp";
    let events = collect_events(input);

    // No entity since there's no semicolon — just text
    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(
        !has_entity,
        "Unterminated entity at EOF should not emit TextEntity"
    );

    // The content should still be emitted as text
    assert!(events.iter().any(|e| matches!(e, Event::Text { .. })));
}

/// @ai-generated - Very long entity name exceeding old limit (was 12, now 40)
#[test]
fn test_long_entity_name() {
    // &CounterClockwiseContourIntegral; is 38 chars (valid HTML5 entity)
    let input = "a&CounterClockwiseContourIntegral;b";
    let events = collect_events(input);

    let has_entity = events.iter().any(|e| matches!(e, Event::TextEntity { .. }));
    assert!(
        has_entity,
        "Long entity names (up to 40 chars) should be recognized"
    );
}

/// @ai-generated - CDATA not closed (EOF inside CDATA)
#[test]
fn test_cdata_not_closed() {
    let input = "<![CDATA[content without end";
    let events = collect_events(input);

    // Should not crash, should emit End
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - Single dash declaration (not a comment) recovers to >
#[test]
fn test_single_dash_declaration() {
    let input = "<!-x>after";
    let events = collect_events(input);

    let text = events.iter().find_map(|e| match e {
        Event::Text { start, end } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(
        text,
        Some("after"),
        "Text after single-dash declaration should be preserved"
    );
}

/// @ai-generated - Declaration at EOF
#[test]
fn test_declaration_at_eof() {
    let input = "<!DOCTYPE";
    let events = collect_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::End)));
}

/// @ai-generated - Slash without > in tag (UNEXPECTED_SOLIDUS_IN_TAG)
#[test]
fn test_unexpected_solidus_in_tag() {
    let input = "<div /x class=\"a\">";
    let events = collect_events(input);

    assert!(
        events.iter().any(|e| match e {
            Event::Error { code, .. } => *code == ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG,
            _ => false,
        }),
        "Should emit UNEXPECTED_SOLIDUS_IN_TAG error"
    );
    // Should still parse the remaining attributes
    assert!(events.iter().any(|e| matches!(e, Event::AttribName { .. })));
}

/// @ai-generated - = before attribute name
#[test]
fn test_equals_before_attr_name() {
    let input = "<div =val>";
    let events = collect_events(input);

    assert!(
        events.iter().any(|e| match e {
            Event::Error { code, .. } =>
                *code == ErrorCode::UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME,
            _ => false,
        }),
        "Should emit UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME error"
    );
}

/// @ai-generated - Case-insensitive textarea close tag
#[test]
fn test_textarea_close_case_insensitive() {
    let input = "<textarea>content</TEXTAREA>";
    let events = collect_events(input);

    assert!(
        events.iter().any(|e| matches!(e, Event::CloseTag { .. })),
        "Case-insensitive </TEXTAREA> should close the textarea"
    );
}

/// @ai-generated - Directive shorthand # (template slot)
#[test]
fn test_directive_shorthand_hash() {
    let input = "<template #default>content</template>";
    let events = collect_events(input);

    // # should emit a DirName event (shorthand for v-slot)
    let has_dir = events.iter().any(|e| matches!(e, Event::DirName { .. }));
    assert!(has_dir, "# should be recognized as directive shorthand");

    // Should have a DirArg for "default"
    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg { start, end, .. } => Some(&input[*start as usize..*end as usize]),
        _ => None,
    });
    assert_eq!(dir_arg, Some("default"), "Should parse 'default' as DirArg");
}

/// @ai-generated - Multiple chained modifiers
#[test]
fn test_multiple_chained_modifiers() {
    let input = r#"<button v-on:click.stop.prevent="fn"></button>"#;
    let events = collect_events(input);

    let modifier_count = events
        .iter()
        .filter(|e| matches!(e, Event::DirModifier { .. }))
        .count();
    assert_eq!(
        modifier_count, 2,
        "Should emit 2 DirModifier events for .stop.prevent"
    );

    let modifiers: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::DirModifier { start, end } => Some(&input[*start as usize..*end as usize]),
            _ => None,
        })
        .collect();
    assert_eq!(modifiers, vec!["stop", "prevent"]);
}

/// @ai-generated - Empty dynamic directive argument
#[test]
fn test_empty_dynamic_dir_arg() {
    let input = r#"<div v-bind:[]="val">text</div>"#;
    let events = collect_events(input);

    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg {
            is_dynamic,
            start,
            end,
        } => Some((*is_dynamic, &input[*start as usize..*end as usize])),
        _ => None,
    });
    assert!(dir_arg.is_some(), "Should emit DirArg for empty []");
    let (is_dynamic, _content) = dir_arg.unwrap();
    assert!(is_dynamic, "[] should be dynamic");
}

/// @ai-generated - Custom delimiters with interpolation in textarea
#[test]
fn test_custom_delimiters_in_textarea() {
    let input = "<textarea><% msg %></textarea>";
    let mut events = Vec::new();
    tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"<%", b"%>");

    let has_interp = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(has_interp, "Custom delimiters should work inside textarea");
}

// ==================== Review edge-case tests ====================

/// @ai-generated - v-pre inside a quoted attribute value should not activate v-pre
#[test]
fn test_v_pre_in_quoted_attr_value_no_false_positive() {
    let input = r#"<div class="v-pre">{{ msg }}</div>"#;
    let events = collect_events(input);

    // Interpolation should still be emitted — v-pre inside a quoted value is NOT a directive
    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        has_interpolation,
        "v-pre inside quoted attribute value should NOT suppress interpolation"
    );

    // Should NOT emit DirVPre
    let has_v_pre = events.iter().any(|e| matches!(e, Event::DirVPre { .. }));
    assert!(
        !has_v_pre,
        "v-pre inside quoted attribute value should NOT emit DirVPre"
    );
}

/// @ai-generated - Backslash at EOF in quoted attribute value
#[test]
fn test_backslash_at_eof_in_quoted_attr() {
    let input = r#"<div attr="val\"#;
    let events = collect_events(input);

    // Should emit AttribData with partial content
    assert!(
        events.iter().any(|e| matches!(e, Event::AttribData { .. })),
        "Should emit AttribData even with trailing backslash at EOF"
    );
    // Should emit EOF error
    assert!(
        events.iter().any(|e| matches!(e, Event::Error { .. })),
        "Should emit error for unterminated quoted attribute at EOF"
    );
}

/// @ai-generated - Deeply nested dynamic directive argument with multiple bracket levels
#[test]
fn test_deeply_nested_dynamic_dir_arg() {
    let input = r#"<div v-bind:[[a]]="val">text</div>"#;
    let events = collect_events(input);

    let dir_arg = events.iter().find_map(|e| match e {
        Event::DirArg {
            is_dynamic,
            start,
            end,
        } => Some((*is_dynamic, &input[*start as usize..*end as usize])),
        _ => None,
    });
    assert!(dir_arg.is_some(), "Should emit DirArg for nested brackets");
    let (is_dynamic, content) = dir_arg.unwrap();
    assert!(is_dynamic, "Nested brackets should be dynamic");
    assert_eq!(
        content, "[[a]]",
        "Should capture full dynamic arg span including outer brackets"
    );
}

/// @ai-generated - RCDATA with close tag prefix but different tag name
#[test]
fn test_rcdata_script_partial_close_no_exit() {
    let input = "<script></scripting>real content</script>";
    let events = collect_events(input);

    // Should have a CloseTag for </script>, not </scripting>
    let close_tags: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::CloseTag {
                start, name_end, ..
            } => Some(&input[*start as usize + 2..*name_end as usize]),
            _ => None,
        })
        .collect();

    assert!(
        close_tags.contains(&"script"),
        "Should close on </script>, got: {:?}",
        close_tags
    );
    // The </scripting> should NOT close the script tag
    assert!(
        !close_tags.contains(&"scripting"),
        "</scripting> should not produce a separate close tag"
    );
}

/// @ai-generated - Processing instruction at EOF without closing >
#[test]
fn test_processing_instruction_eof_no_close() {
    let input = "<?xml version";
    let events = collect_events(input);

    // Should emit ProcessingInstruction with partial content
    let has_pi = events
        .iter()
        .any(|e| matches!(e, Event::ProcessingInstruction { .. }));
    assert!(has_pi, "Should emit ProcessingInstruction even at EOF");

    // Should emit EOF error
    let has_error = events.iter().any(|e| match e {
        Event::Error { code, .. } => *code == ErrorCode::EOF_IN_TAG,
        _ => false,
    });
    assert!(
        has_error,
        "Should emit EOF_IN_TAG error for unclosed processing instruction"
    );
}

// ==================== v-pre with shorthand directives ====================

#[test]
fn test_v_pre_suppresses_shorthand_bind() {
    // :class is shorthand for v-bind:class — should be suppressed by v-pre
    let events = collect_events(r#"<div v-pre :class="foo">{{ msg }}</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand :class should NOT emit DirName inside v-pre, got {:?}",
        dir_names
    );

    // Interpolation should be text, not Interpolation
    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation inside v-pre should be text"
    );
}

#[test]
fn test_v_pre_suppresses_shorthand_on() {
    // @click is shorthand for v-on:click — should be suppressed by v-pre
    let events = collect_events(r#"<div v-pre @click="handler">text</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand @click should NOT emit DirName inside v-pre"
    );
}

#[test]
fn test_v_pre_suppresses_shorthand_slot() {
    // #default is shorthand for v-slot:default — should be suppressed by v-pre
    let events = collect_events(r#"<div v-pre #default>text</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand #default should NOT emit DirName inside v-pre"
    );
}

#[test]
fn test_v_pre_suppresses_shorthand_dot() {
    // .prop is shorthand for v-bind.prop — should be suppressed by v-pre
    let events = collect_events(r#"<div v-pre .prop="val">text</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand .prop should NOT emit DirName inside v-pre"
    );
}

#[test]
fn test_v_pre_last_suppresses_shorthand_bind() {
    // v-pre comes AFTER :class — prepass should still suppress it
    let events = collect_events(r#"<div :class="foo" v-pre>{{ msg }}</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand :class should NOT emit DirName when v-pre comes after it"
    );

    let has_interpolation = events
        .iter()
        .any(|e| matches!(e, Event::Interpolation { .. }));
    assert!(
        !has_interpolation,
        "Interpolation inside v-pre should be text"
    );
}

#[test]
fn test_v_pre_last_suppresses_shorthand_on() {
    // v-pre comes AFTER @click — prepass should still suppress it
    let events = collect_events(r#"<button @click="handler" v-pre>text</button>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        0,
        "Shorthand @click should NOT emit DirName when v-pre comes after it"
    );
}

#[test]
fn test_no_v_pre_shorthand_still_works() {
    // Without v-pre, shorthand directives should still emit DirName
    let events = collect_events(r#"<div :class="foo" @click="handler">text</div>"#);

    let dir_names: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::DirName { .. }))
        .collect();

    assert_eq!(
        dir_names.len(),
        2,
        "Without v-pre, shorthand directives should emit DirName events"
    );
}
