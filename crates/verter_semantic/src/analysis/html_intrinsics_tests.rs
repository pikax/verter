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

// ── Events have payloads (recovered from the catalog by id) ─────────────

#[test]
fn events_have_payloads() {
    let catalog = html_intrinsic_catalog();
    let members = intrinsic_members_for_tag("div");
    let listener_shape = |event: &str| -> &IntrinsicTypeShape {
        let member = members
            .iter()
            .find(|m| m.kind == IntrinsicMemberKind::Listener && m.name == event)
            .unwrap_or_else(|| panic!("div must have {event} listener"));
        catalog
            .shape(member.type_id)
            .unwrap_or_else(|| panic!("{event} id must resolve in the catalog"))
    };

    match listener_shape("click") {
        IntrinsicTypeShape::ListenerFunction(display) => {
            assert!(
                display.contains("PointerEvent"),
                "click should have PointerEvent payload, got: {display}"
            );
            assert!(
                display.contains("=>"),
                "listener shape is a normalized function form, got: {display}"
            );
        }
        other => panic!("expected ListenerFunction shape for click, got: {other:?}"),
    }
    match listener_shape("focus") {
        IntrinsicTypeShape::ListenerFunction(display) => {
            assert!(
                display.contains("FocusEvent"),
                "focus should have FocusEvent payload, got: {display}"
            );
        }
        other => panic!("expected ListenerFunction shape for focus, got: {other:?}"),
    }
    match listener_shape("keydown") {
        IntrinsicTypeShape::ListenerFunction(display) => {
            assert!(
                display.contains("KeyboardEvent"),
                "keydown should have KeyboardEvent payload, got: {display}"
            );
        }
        other => panic!("expected ListenerFunction shape for keydown, got: {other:?}"),
    }
}

// ── Catalog determinism + id/shape recovery ─────────────────────────────

#[test]
fn catalog_ids_are_deterministic_and_recover_shapes() {
    let catalog = html_intrinsic_catalog();
    assert!(!catalog.is_empty(), "generated data populates the catalog");

    // Attr primitives fold to Primitive shapes; non-primitive attr display
    // text is preserved verbatim.
    let members = intrinsic_members_for_tag("div");
    let id_attr = members
        .iter()
        .find(|m| m.kind == IntrinsicMemberKind::Attr && m.name == "id")
        .expect("div id attr");
    assert_eq!(
        catalog.shape(id_attr.type_id),
        Some(&IntrinsicTypeShape::Primitive(
            verter_type_expr::PrimitiveName::String
        ))
    );
    let draggable = members
        .iter()
        .find(|m| m.kind == IntrinsicMemberKind::Attr && m.name == "draggable")
        .expect("div draggable attr");
    match catalog.shape(draggable.type_id).expect("draggable shape") {
        IntrinsicTypeShape::AttrDisplay(display) => {
            assert!(
                display.contains("Booleanish"),
                "non-primitive attr keeps its generated display text, got {display}"
            );
        }
        other => panic!("expected AttrDisplay for draggable, got {other:?}"),
    }

    // Determinism: repeated queries mint IDENTICAL ids (same shape ⇒ same id),
    // and every member id resolves in the catalog (no dangling ordinals).
    let again = intrinsic_members_for_tag("div");
    assert_eq!(members, again, "member ids are stable across queries");
    for member in &members {
        assert!(
            catalog.shape(member.type_id).is_some(),
            "member {} carries a dangling catalog id",
            member.name
        );
    }

    // Dedup: two members with the SAME shape share one id (`id` and `title`
    // are both generated as plain "string" attrs); a DIFFERENT shape gets a
    // different id (`class` is generated as "any", not "string").
    let title_attr = members
        .iter()
        .find(|m| m.kind == IntrinsicMemberKind::Attr && m.name == "title")
        .expect("div title attr");
    assert_eq!(
        title_attr.type_id, id_attr.type_id,
        "equal shapes intern to one id"
    );
    let class_attr = members
        .iter()
        .find(|m| m.kind == IntrinsicMemberKind::Attr && m.name == "class")
        .expect("div class attr");
    assert_ne!(
        class_attr.type_id, id_attr.type_id,
        "distinct shapes (any vs string) must not collapse to one id"
    );
    assert_eq!(
        catalog.shape(class_attr.type_id),
        Some(&IntrinsicTypeShape::AttrDisplay("any".to_string())),
        "the non-primitive `any` display text is preserved verbatim"
    );

    // Owned facts carry the same content-free ids.
    let owned = owned_intrinsic_members_for_tag("div");
    let owned_id = owned
        .iter()
        .find(|m| m.name == "id")
        .expect("owned id attr");
    assert_eq!(owned_id.type_id, id_attr.type_id);
    assert_eq!(owned_id.kind, IntrinsicMemberKind::Attr);
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
