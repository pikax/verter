//! @ai-generated - Synthetic message-list-like typeinfo fixture tests.

use super::support::*;

#[test]
fn message_list_like_extracts_direct_pick_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/message-list-like.ts", MESSAGE_LIST_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/message-list-like.ts",
        "ConcreteMessageDirectUserProps",
        &[],
        ProjectionMode::Expanded,
    );

    // Fixture: MessageProps<M, D, U> declares icon/avatar/variant/side/actions/ui all optional.
    // ConcreteMessageDirectUserProps = Pick<MessageProps<...>, "icon"|"avatar"|"variant"|"side"|"actions"|"ui">.
    // TS7 keeps the optional markers through Pick.
    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["actions", "avatar", "icon", "side", "ui", "variant"]
    );
    assert!(props["actions"].optional);
    assert_array_of_ref(&props["actions"].ty, "ActionItem");
    assert!(props["avatar"].optional);
    assert_ref(&props["avatar"].ty, "AvatarConfig");
    assert!(props["icon"].optional);
    assert_primitive(&props["icon"].ty, PrimitiveName::String);
    assert!(props["side"].optional);
    assert_literal_union(&props["side"].ty, &["left", "right"]);
    assert!(props["ui"].optional);
    assert_ref(&props["ui"].ty, "MessageUi");
    assert!(props["variant"].optional);
    assert_literal_union(&props["variant"].ty, &["naked", "soft", "solid"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn message_list_like_direct_slot_payload_keeps_message_context() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/message-list-like.ts", MESSAGE_LIST_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/message-list-like.ts",
        "ConcreteMessageDirectContentPayload",
        &[],
        ProjectionMode::Expanded,
    );

    // Fixture: ConcreteMessageDirectContentPayload = { compact: boolean } & { message: ConcreteMessage }.
    // TS7: both members required; intersection materializes both required.
    let payload = object_props(&expr);
    assert_eq!(prop_names(&payload), vec!["compact", "message"]);
    assert!(!payload["message"].optional);
    assert_ref(&payload["message"].ty, "ConcreteMessage");
    assert!(!payload["compact"].optional);
    assert_primitive(&payload["compact"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Pick<PropsBase<T[number] infer ...>> as semanticMiss; keep as the future inferred-array-element contract"]
fn message_list_like_extracts_pick_from_inferred_array_element() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/message-list-like.ts", MESSAGE_LIST_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/message-list-like.ts",
        "ConcreteMessageListUserProps",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["actions", "avatar", "icon", "side", "ui", "variant"]
    );
    assert_array_of_ref(&props["actions"].ty, "ActionItem");
    assert_ref(&props["ui"].ty, "MessageUi");
    assert_literal_union(&props["side"].ty, &["left", "right"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves mapped slot remapping over inferred message context as semanticMiss; keep as the future message-slot remap contract"]
fn message_list_like_slot_remaps_payload_with_message_context() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/message-list-like.ts", MESSAGE_LIST_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/message-list-like.ts",
        "ConcreteMessageContentSlotPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let payload = object_props(&expr);
    assert_ref(&payload["message"].ty, "ConcreteMessage");
    assert_primitive(&payload["compact"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
