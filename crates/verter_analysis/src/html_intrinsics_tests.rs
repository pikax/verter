use super::*;

// ── Global attrs ────────────────────────────────────────────────────────

#[test]
fn div_has_global_attrs() {
    let members = intrinsic_members_for_tag("div");
    let attr_names: Vec<&str> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Attr)
        .map(|m| m.name)
        .collect();

    // Positive: key global attrs must be present
    assert!(attr_names.contains(&"class"), "div must have class attr");
    assert!(attr_names.contains(&"style"), "div must have style attr");
    assert!(attr_names.contains(&"id"), "div must have id attr");
    assert!(
        attr_names.contains(&"tabindex"),
        "div must have tabindex attr"
    );
    assert!(attr_names.contains(&"title"), "div must have title attr");
    assert!(attr_names.contains(&"role"), "div must have role attr");
    assert!(
        attr_names.contains(&"draggable"),
        "div must have draggable attr"
    );
    assert!(attr_names.contains(&"hidden"), "div must have hidden attr");
}

// ── Input-specific attrs ────────────────────────────────────────────────

#[test]
fn input_has_specific_attrs() {
    let members = intrinsic_members_for_tag("input");
    let attr_names: Vec<&str> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Attr)
        .map(|m| m.name)
        .collect();

    // Positive: input-specific attrs
    assert!(attr_names.contains(&"type"), "input must have type attr");
    assert!(attr_names.contains(&"value"), "input must have value attr");
    assert!(
        attr_names.contains(&"checked"),
        "input must have checked attr"
    );
    assert!(
        attr_names.contains(&"placeholder"),
        "input must have placeholder attr"
    );
    assert!(
        attr_names.contains(&"disabled"),
        "input must have disabled attr"
    );
    assert!(
        attr_names.contains(&"readonly"),
        "input must have readonly attr"
    );
    assert!(
        attr_names.contains(&"maxlength"),
        "input must have maxlength attr"
    );
    assert!(attr_names.contains(&"min"), "input must have min attr");
    assert!(attr_names.contains(&"max"), "input must have max attr");
    assert!(attr_names.contains(&"step"), "input must have step attr");
    assert!(
        attr_names.contains(&"pattern"),
        "input must have pattern attr"
    );

    // Also has global attrs
    assert!(
        attr_names.contains(&"id"),
        "input must also have global id attr"
    );
    assert!(
        attr_names.contains(&"class"),
        "input must also have global class attr"
    );
}

// ── Events have payloads ────────────────────────────────────────────────

#[test]
fn events_have_payloads() {
    let members = intrinsic_members_for_tag("div");
    let listeners: Vec<(&str, &TypeExpr)> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Listener)
        .map(|m| (m.name, &m.type_expr))
        .collect();

    // Find click event
    let click = listeners.iter().find(|(name, _)| *name == "click");
    assert!(click.is_some(), "div must have click listener");
    let (_, click_type) = click.unwrap();
    match click_type {
        TypeExpr::Unknown { raw } => {
            assert!(
                raw.contains("PointerEvent"),
                "click should have PointerEvent payload, got: {}",
                raw
            );
        }
        other => panic!("expected Unknown type for click, got: {:?}", other),
    }

    // Find focus event
    let focus = listeners.iter().find(|(name, _)| *name == "focus");
    assert!(focus.is_some(), "div must have focus listener");
    let (_, focus_type) = focus.unwrap();
    match focus_type {
        TypeExpr::Unknown { raw } => {
            assert!(
                raw.contains("FocusEvent"),
                "focus should have FocusEvent payload, got: {}",
                raw
            );
        }
        other => panic!("expected Unknown type for focus, got: {:?}", other),
    }

    // Find keydown event
    let keydown = listeners.iter().find(|(name, _)| *name == "keydown");
    assert!(keydown.is_some(), "div must have keydown listener");
    let (_, keydown_type) = keydown.unwrap();
    match keydown_type {
        TypeExpr::Unknown { raw } => {
            assert!(
                raw.contains("KeyboardEvent"),
                "keydown should have KeyboardEvent payload, got: {}",
                raw
            );
        }
        other => panic!("expected Unknown type for keydown, got: {:?}", other),
    }
}

// ── Unknown/custom tags get only global surface ─────────────────────────

#[test]
fn unknown_tag_uses_global_surface() {
    let known = intrinsic_members_for_tag("div");
    let unknown = intrinsic_members_for_tag("my-custom-element");

    // Unknown tag should have exactly the same members as a generic div
    // (all global, no tag-specific)
    assert_eq!(
        known.len(),
        unknown.len(),
        "unknown tag should have same member count as div (both global-only)"
    );

    // Negative: unknown tag must NOT have input-specific attrs
    let attr_names: Vec<&str> = unknown
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Attr)
        .map(|m| m.name)
        .collect();
    assert!(
        !attr_names.contains(&"checked"),
        "unknown tag must NOT have input-specific 'checked'"
    );
    assert!(
        !attr_names.contains(&"maxlength"),
        "unknown tag must NOT have input-specific 'maxlength'"
    );
}

// ── DOM instance members absent ─────────────────────────────────────────

#[test]
fn dom_instance_members_absent() {
    let members = intrinsic_members_for_tag("div");
    let all_names: Vec<&str> = members.iter().map(|m| m.name).collect();

    // These are DOM instance properties, not valid Vue template attrs
    assert!(
        !all_names.contains(&"offsetWidth"),
        "offsetWidth is DOM-only, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"offsetHeight"),
        "offsetHeight is DOM-only, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"clientWidth"),
        "clientWidth is DOM-only, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"scrollTop"),
        "scrollTop is DOM-only, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"textContent"),
        "textContent is DOM-only, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"childNodes"),
        "childNodes is DOM-only, must not be in intrinsics"
    );
}

// ── DOM methods absent ──────────────────────────────────────────────────

#[test]
fn dom_methods_absent() {
    let members = intrinsic_members_for_tag("div");
    // Check ATTR names only — DOM methods should not be attrs
    let attr_names: Vec<&str> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Attr)
        .map(|m| m.name)
        .collect();

    assert!(
        !attr_names.contains(&"focus"),
        "focus() is a method, must not be in attr intrinsics"
    );
    assert!(
        !attr_names.contains(&"blur"),
        "blur() is a method, must not be in attr intrinsics"
    );
    assert!(
        !attr_names.contains(&"querySelector"),
        "querySelector is a method, must not be in intrinsics"
    );
    assert!(
        !attr_names.contains(&"addEventListener"),
        "addEventListener is a method, must not be in intrinsics"
    );
    assert!(
        !attr_names.contains(&"removeEventListener"),
        "removeEventListener is a method, must not be in intrinsics"
    );

    // But "focus" and "blur" as LISTENERS (event names) SHOULD be present
    let listener_names: Vec<&str> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Listener)
        .map(|m| m.name)
        .collect();
    assert!(
        listener_names.contains(&"focus"),
        "focus as a listener event must be present"
    );
    assert!(
        listener_names.contains(&"blur"),
        "blur as a listener event must be present"
    );
}

// ── Directive pseudo-members absent ─────────────────────────────────────

#[test]
fn directive_pseudo_members_absent() {
    let members = intrinsic_members_for_tag("div");
    let all_names: Vec<&str> = members.iter().map(|m| m.name).collect();

    assert!(
        !all_names.contains(&"v-slot"),
        "v-slot is a directive pseudo-member, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"v-directive"),
        "v-directive is a directive pseudo-member, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"v-if"),
        "v-if is a directive, must not be in intrinsics"
    );
    assert!(
        !all_names.contains(&"v-for"),
        "v-for is a directive, must not be in intrinsics"
    );
}

// ── innerHTML excluded ──────────────────────────────────────────────────

#[test]
fn inner_html_excluded() {
    // innerHTML is listed in Vue's HTMLAttributes but it's a DOM property,
    // not a valid Vue template attr for fallthrough purposes.
    let members = intrinsic_members_for_tag("div");
    let attr_names: Vec<&str> = members
        .iter()
        .filter(|m| m.kind == IntrinsicMemberKind::Attr)
        .map(|m| m.name)
        .collect();
    assert!(
        !attr_names.contains(&"innerHTML"),
        "innerHTML must NOT be in the intrinsic catalog (it's a DOM property)"
    );
}

// ── Event name / onXxx conversion ───────────────────────────────────────

#[test]
fn event_name_to_on_prop_conversion() {
    assert_eq!(event_name_to_on_prop("click"), "onClick");
    assert_eq!(event_name_to_on_prop("mousedown"), "onMousedown");
    assert_eq!(event_name_to_on_prop("focus"), "onFocus");
    assert_eq!(
        event_name_to_on_prop("update:modelValue"),
        "onUpdate:modelValue"
    );
}

#[test]
fn on_prop_to_event_name_conversion() {
    assert_eq!(on_prop_to_event_name("onClick"), Some("click".to_string()));
    assert_eq!(
        on_prop_to_event_name("onMousedown"),
        Some("mousedown".to_string())
    );
    assert_eq!(on_prop_to_event_name("onFocus"), Some("focus".to_string()));
    // Not a valid onXxx form
    assert_eq!(on_prop_to_event_name("onclick"), None);
    assert_eq!(on_prop_to_event_name("on"), None);
    assert_eq!(on_prop_to_event_name("disabled"), None);
}
