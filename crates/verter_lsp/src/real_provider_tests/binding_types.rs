//! `$/verter/getBindingTypes` real-provider coverage (P1-01) plus the
//! engine-split `display_signature`/`kind` asymmetry pin (ruling Q2).
//!
//! Both engines must populate `display_signature` wherever they answer at all:
//! tsserver from the `quickinfo` wire's `displayString` (markdown-sourced
//! rendering stays in `contents`), tsgo from its plaintext hover block. The
//! `kind` field is tsserver-only — LSP `textDocument/hover` carries no kind and
//! fabricating one is forbidden — so `kind: None` on tsgo is the ACCEPTED,
//! intended asymmetry, pinned here so it can never be read as accidental.

use crate::test_harness::{real_provider_test, RealProviderTestSession};

/// Retry a direct provider hover while the project warms up.
async fn hover_until_some(
    session: &RealProviderTestSession,
    provider_path: &str,
    offset: u32,
) -> Option<verter_type_runtime::protocol::HoverInfo> {
    for attempt in 0..8 {
        if let Ok(Some(info)) = session.provider().get_hover(provider_path, offset).await {
            return Some(info);
        }
        if attempt < 7 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// P1-01: getBindingTypes populates display signatures on BOTH engines
// ---------------------------------------------------------------------------

real_provider_test!(
    binding_types_populate_display_signature,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.require_or_skip_ready(&uri, "action.disabled", 7, "disabled").await {
            return;
        }

        let value = session
            .server()
            .get_binding_types(crate::server::protocol_types::GetAnalysisParams {
                uri: uri.as_str().to_string(),
            })
            .await
            .expect("getBindingTypes request should succeed");
        let map = value
            .as_object()
            .expect("getBindingTypes returns a JSON object");

        // `count` and `doubled` are script bindings of the fixture (`title` is a
        // defineProps member, NOT an `analysis.bindings` row — deliberately not
        // asserted). Each must carry a non-null, type-bearing display signature.
        for (name, type_token) in [("count", "number"), ("doubled", "number")] {
            let entry = map
                .get(name)
                .unwrap_or_else(|| panic!("binding {name} must be present, got: {value}"));
            let signature = entry
                .get("displaySignature")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| {
                    panic!("binding {name} must carry a displaySignature, got: {entry}")
                });

            assert!(
                signature.contains(type_token),
                "{name}'s display signature should mention {type_token}, got: {signature}"
            );
            // Negatives: the wire value is a display signature, never rendered
            // markdown and never the bare binding name echoed back.
            assert!(
                !signature.contains("```"),
                "{name}'s display signature must not carry a markdown fence: {signature}"
            );
            assert!(
                !signature.contains("typescript"),
                "{name}'s display signature must not carry the fence language tag: {signature}"
            );
            assert_ne!(
                signature, name,
                "{name}'s display signature must not be the bare binding name"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// Engine-split pin (ruling Q2 condition 3): display_signature is populated on
// BOTH engines; `kind` is tsserver-only and stays None on tsgo (never
// fabricated, never prefix-sniffed).
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_display_signature_engine_split,
    fixture = "single-project",
    async fn run(session) {
        // A direct provider file keeps this pin at the provider protocol layer
        // (the asymmetry is a producer contract, not an LSP-merge behavior).
        let source = "export const answer: number = 42;\n";
        let path = session
            .open_in_provider("src/__binding_types_probe.ts", source)
            .await;

        // Hover on `answer` in the declaration.
        let offset = (source.find("answer").expect("needle present") + 1) as u32;
        let Some(info) = hover_until_some(session, &path, offset).await else {
            if session.allow_empty_result_skip(&format!(
                "provider returned no hover for the probe const at offset {offset}"
            )) {
                return;
            }
            unreachable!("allow_empty_result_skip panics under require-mode");
        };

        let signature = info
            .display_signature
            .as_ref()
            .expect("both engines must populate display_signature where they answer at all")
            .as_display_str();
        assert!(
            signature.contains("answer") && signature.contains("number"),
            "display signature should describe the probe const, got: {signature}"
        );
        assert!(
            !signature.contains("```"),
            "display signature is plaintext, never fenced: {signature}"
        );

        if session.is_tsgo() {
            // ACCEPTED ASYMMETRY: LSP hover has no kind field; fabricating one
            // (or sniffing it from display-string prefixes) is forbidden (F-04).
            assert!(
                info.kind.is_none(),
                "tsgo must not fabricate a quick-info kind, got: {:?}",
                info.kind
            );
        } else {
            assert!(
                info.kind.is_some(),
                "tsserver populates the structured kind from the quickinfo wire"
            );
        }
    }
);
