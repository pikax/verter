//! Pure, fast codec + version-gate tests for the control-protocol wire contract.
//!
//! Every message round-trips through serde_json byte-for-byte-stable, the wire
//! field names are asserted to be the exact camelCase strings a TS client
//! reads, and the version gate is proven to fail closed on a wrong protocol or
//! nonce (the discriminating negatives).

use super::*;

/// A message type round-trips: serialize → deserialize → equal to the original.
fn round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_value(value).expect("serialize");
    let back: T = serde_json::from_value(json).expect("deserialize");
    assert_eq!(&back, value, "round-trip must preserve the value");
}

#[test]
fn protocol_version_is_pinned() {
    // A deliberate pin: bumping this is a conscious wire-breaking change.
    assert_eq!(
        PROTOCOL_VERSION, 2,
        "PROTOCOL_VERSION is a stable pin; a change must be deliberate + version-gated"
    );
}

#[test]
fn method_names_are_the_stable_wire_strings() {
    // The method-name strings ARE the wire contract; assert them verbatim so a
    // rename is caught here (a TS client hard-codes these).
    assert_eq!(METHOD_HELLO, "verter/hello");
    assert_eq!(METHOD_WAIT_INITIALIZED, "verter/waitInitialized");
    assert_eq!(
        METHOD_CARRIER_DID_OPEN_SYNCED,
        "verter/carrierDidOpenSynced"
    );
    assert_eq!(
        METHOD_CARRIER_DID_CHANGE_SYNCED,
        "verter/carrierDidChangeSynced"
    );
    assert_eq!(METHOD_CARRIER_DID_CLOSE, "verter/carrierDidClose");
    assert_eq!(METHOD_INITIALIZE_API_SESSION, "verter/initializeApiSession");
    assert_eq!(METHOD_FEATURE_REQUEST, "verter/featureRequest");
    assert_eq!(METHOD_DETACH, "verter/detach");
    assert_eq!(METHOD_STATUS, "verter/status");
    assert_eq!(METHOD_FATAL, "verter/fatal");
}

#[test]
fn hello_messages_round_trip() {
    round_trip(&HelloParams {
        protocol: PROTOCOL_VERSION,
        nonce: "abc123".to_string(),
        client: "verter_lsp".to_string(),
    });
    round_trip(&HelloResult {
        protocol: PROTOCOL_VERSION,
        session_id: "ctl-7".to_string(),
        wire_pin: 0xDEAD_BEEF,
        editor_session_generation: 42,
        capabilities: ControlCapabilities {
            carrier_injection: true,
            api_session: true,
            wait_initialized: true,
            feature_requests: true,
        },
    });
}

#[test]
fn feature_request_round_trips_as_a_typed_closed_surface() {
    let request = FeatureRequestParams {
        method: FeatureRequestMethod::Hover,
        params: serde_json::json!({
            "textDocument": { "uri": "file:///w/Comp.vue.tsx" },
            "position": { "line": 2, "character": 7 }
        }),
    };
    round_trip(&request);
    assert_eq!(
        serde_json::to_value(request.method).unwrap(),
        serde_json::json!("textDocument/hover"),
        "the typed enum serializes to the exact upstream LSP method"
    );
    round_trip(&FeatureRequestResult {
        result: serde_json::json!({
            "contents": { "kind": "markdown", "value": "```ts\nconst label: string\n```" }
        }),
    });
}

#[test]
fn pull_diagnostics_is_in_the_closed_read_only_feature_set() {
    let method = FeatureRequestMethod::Diagnostic;
    assert_eq!(method.as_lsp_method(), "textDocument/diagnostic");
    assert_eq!(
        FeatureRequestMethod::from_lsp_method("textDocument/diagnostic"),
        Some(method)
    );
}

#[test]
fn hello_result_uses_camel_case_wire_fields() {
    let value = serde_json::to_value(HelloResult {
        protocol: 1,
        session_id: "s".to_string(),
        wire_pin: 9,
        editor_session_generation: 3,
        capabilities: ControlCapabilities::default(),
    })
    .unwrap();
    let obj = value.as_object().unwrap();
    // The exact camelCase keys a TS client reads.
    assert!(obj.contains_key("sessionId"), "sessionId (camelCase)");
    assert!(obj.contains_key("wirePin"), "wirePin (camelCase)");
    assert!(
        obj.contains_key("editorSessionGeneration"),
        "editorSessionGeneration (camelCase)"
    );
    // Negative: the snake_case forms must NOT appear.
    assert!(!obj.contains_key("session_id"), "no snake_case session_id");
    assert!(!obj.contains_key("wire_pin"), "no snake_case wire_pin");
}

#[test]
fn wait_initialized_result_round_trips_and_is_camel_case() {
    let result = WaitInitializedResult {
        server_info_version: Some("7.0.1-rc".to_string()),
        observed_initialize_id: serde_json::json!(7),
        root_uri: Some("file:///w".to_string()),
        workspace_folders: Some(serde_json::json!([{ "uri": "file:///w", "name": "w" }])),
    };
    round_trip(&result);
    let obj = serde_json::to_value(&result).unwrap();
    let obj = obj.as_object().unwrap();
    assert!(obj.contains_key("serverInfoVersion"));
    assert!(obj.contains_key("observedInitializeId"));
    assert!(obj.contains_key("rootUri"));
    assert!(obj.contains_key("workspaceFolders"));
}

#[test]
fn carrier_lifecycle_params_round_trip_and_are_camel_case() {
    let open = CarrierDidOpenSyncedParams {
        uri: "file:///c.ts".to_string(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const x = 1;".to_string(),
    };
    round_trip(&open);
    let obj = serde_json::to_value(&open).unwrap();
    assert!(obj.as_object().unwrap().contains_key("languageId"));
    assert!(!obj.as_object().unwrap().contains_key("language_id"));

    round_trip(&CarrierDidChangeSyncedParams {
        uri: "file:///c.ts".to_string(),
        version: 2,
        text: "export const x = 2;".to_string(),
    });
    round_trip(&CarrierDidCloseParams {
        uri: "file:///c.ts".to_string(),
    });
}

#[test]
fn initialize_api_session_result_serializes_exactly_one_endpoint() {
    // Windows variant: only pipeName is present.
    let win = InitializeApiSessionResult {
        pipe_name: Some(r"\\.\pipe\tsgo-api-1".to_string()),
        socket_path: None,
        wire_pin: 5,
        handle_kind: "integer".to_string(),
    };
    round_trip(&win);
    let obj = serde_json::to_value(&win).unwrap();
    let obj = obj.as_object().unwrap();
    assert!(obj.contains_key("pipeName"));
    // The absent Unix variant is elided (skip_serializing_if), not `null`.
    assert!(!obj.contains_key("socketPath"), "absent endpoint is elided");
    assert!(obj.contains_key("wirePin"));
    assert!(obj.contains_key("handleKind"));
    assert_eq!(win.endpoint(), Some(r"\\.\pipe\tsgo-api-1"));

    // Unix variant: only socketPath.
    let nix = InitializeApiSessionResult {
        pipe_name: None,
        socket_path: Some("/tmp/tsgo-api.sock".to_string()),
        wire_pin: 5,
        handle_kind: "integer".to_string(),
    };
    round_trip(&nix);
    let obj = serde_json::to_value(&nix).unwrap();
    assert!(!obj.as_object().unwrap().contains_key("pipeName"));
    assert!(obj.as_object().unwrap().contains_key("socketPath"));
    assert_eq!(nix.endpoint(), Some("/tmp/tsgo-api.sock"));
}

#[test]
fn detach_params_omitted_is_unspecified_not_an_opt_out() {
    // FAIL CLOSED: a `verter/detach` with no params body (`{}`) deserializes to
    // `close_carriers: None` (UNSPECIFIED) — NOT an explicit `Some(false)` opt-out. The
    // server treats `None` (and every malformed body) as RETRACT; only an explicit
    // `closeCarriers: false` opts out of the unified session-end drain.
    let omitted: DetachParams = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(
        omitted.close_carriers, None,
        "an omitted closeCarriers must read as None (unspecified → fail closed = retract)"
    );
    assert_ne!(
        omitted.close_carriers,
        Some(false),
        "an omitted param must NOT read as the explicit closeCarriers:false opt-out"
    );
    // Both explicit values round-trip and emit the camelCase wire field.
    round_trip(&DetachParams {
        close_carriers: Some(false),
    });
    round_trip(&DetachParams {
        close_carriers: Some(true),
    });
    let obj = serde_json::to_value(DetachParams {
        close_carriers: Some(true),
    })
    .unwrap();
    assert!(obj.as_object().unwrap().contains_key("closeCarriers"));
}

#[test]
fn status_and_fatal_round_trip() {
    round_trip(&StatusResult {
        protocol: 1,
        hello_completed: true,
        initialized: true,
        open_carriers: 2,
        api_session_active: false,
    });
    round_trip(&FatalParams {
        reason: FatalReason::ServerExit,
        detail: "tsgo exited".to_string(),
    });
    // The fatal reason is a camelCase discriminant.
    let v = serde_json::to_value(FatalReason::RelayDeath).unwrap();
    assert_eq!(v, serde_json::json!("relayDeath"));
    let v = serde_json::to_value(FatalReason::ProtocolMismatch).unwrap();
    assert_eq!(v, serde_json::json!("protocolMismatch"));
}

#[test]
fn verify_hello_accepts_matching_protocol_and_nonce() {
    let params = HelloParams {
        protocol: PROTOCOL_VERSION,
        nonce: "the-nonce".to_string(),
        client: "verter_lsp".to_string(),
    };
    assert_eq!(verify_hello(&params, "the-nonce"), Ok(()));
}

#[test]
fn verify_hello_rejects_wrong_protocol_fail_closed() {
    // The discriminating version-gate negative: a wrong protocol is refused,
    // and refused AS a protocol mismatch even when the nonce is also wrong.
    let params = HelloParams {
        protocol: PROTOCOL_VERSION + 99,
        nonce: "wrong".to_string(),
        client: "verter_lsp".to_string(),
    };
    let rejection = verify_hello(&params, "the-nonce").expect_err("wrong protocol must be refused");
    assert_eq!(
        rejection,
        HelloRejection::ProtocolMismatch {
            expected: PROTOCOL_VERSION,
            got: PROTOCOL_VERSION + 99,
        }
    );
    assert_eq!(rejection.error_code(), ERROR_PROTOCOL_MISMATCH);
}

#[test]
fn verify_hello_rejects_wrong_nonce_fail_closed() {
    // The discriminating rendezvous negative: a right protocol but wrong nonce
    // is refused (the client did not read THIS shim's advertisement).
    let params = HelloParams {
        protocol: PROTOCOL_VERSION,
        nonce: "stale-or-spoofed".to_string(),
        client: "verter_lsp".to_string(),
    };
    let rejection =
        verify_hello(&params, "the-real-nonce").expect_err("wrong nonce must be refused");
    assert_eq!(rejection, HelloRejection::NonceMismatch);
    assert_eq!(rejection.error_code(), ERROR_NONCE_MISMATCH);
}
