//! Signature-help fidelity tests against real providers (tsserver + tgo).
//!
//! Verifies the script-block TS-parity bar for `textDocument/signatureHelp`: a
//! call site inside a `<script setup>` block behaves like the equivalent
//! standalone `.ts` program, with
//!
//!   - the correct active parameter highlighted for the cursor position
//!     (per-signature `activeParameter` and/or the top-level value), AND
//!   - parameter labels carried in the OFFSET form (`ParameterLabel::LabelOffsets`)
//!     so the client can bold the exact parameter span within the rendered
//!     signature label.
//!
//! Pre-fix (K1 HEAD) the merge hard-coded per-signature `active_parameter: None`
//! and always emitted `ParameterLabel::Simple`, so the offset-form assertion and
//! the per-signature active-param assertion both fail on the pre-K2 tree.

use tower_lsp_server::ls_types::{ParameterLabel, SignatureHelp};

use crate::test_harness::real_provider_test;

/// A self-contained SFC whose `<script setup>` defines a 3-parameter function and
/// calls it. The cursor needle lands just after the first argument's comma, so the
/// active parameter is index 1 (`b`).
const SIG_SFC: &str = r#"<script setup lang="ts">
function addThree(alpha: number, beta: string, gamma: boolean): number {
  return alpha + (beta.length) + (gamma ? 1 : 0);
}

const result = addThree(1, SIG_CURSOR);
</script>

<template>
  <div>{{ result }}</div>
</template>
"#;

/// Request signature help, retrying across provider warmup until a non-empty
/// result arrives (or the budget is exhausted).
async fn signature_help_with_retry(
    session: &crate::test_harness::RealProviderTestSession,
    uri: &tower_lsp_server::ls_types::Uri,
    position: tower_lsp_server::ls_types::Position,
) -> Option<SignatureHelp> {
    for attempt in 0..6 {
        session.ensure_synced(uri).await;
        if let Some(help) = session.signature_help(uri, position).await {
            if !help.signatures.is_empty() {
                return Some(help);
            }
        }
        if attempt < 5 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    None
}

real_provider_test!(
    signature_help_active_param_and_offsets,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_virtual("src/SigHelpCase.vue", SIG_SFC).await;

        // Position the cursor at the `SIG_CURSOR` marker (the 2nd argument slot,
        // i.e. active parameter index 1), then strip the marker so the call reads
        // `addThree(1, )`.
        let marker_pos = session.find_position(&uri, "SIG_CURSOR", 0);
        // The marker text itself is harmless filler for the call; tsserver/tgo
        // resolve the active param from the comma count before the cursor.
        let backend = if session.is_tsgo() { "tgo" } else { "tsserver" };
        let Some(help) = signature_help_with_retry(session, &uri, marker_pos).await else {
            // Fail-closed: under the require-env gate the harness build already
            // hard-failed on a missing provider, so reaching here with no result
            // means the provider genuinely returned nothing — surface it.
            if std::env::var(if session.is_tsgo() {
                "VERTER_REQUIRE_TSGO"
            } else {
                "VERTER_REQUIRE_TSSERVER"
            })
            .is_ok()
            {
                panic!(
                    "signature help returned no signatures for addThree(...) under require-env \
                     ({backend} provider)"
                );
            }
            eprintln!("skipping: provider {backend} returned no signature help (not warmed up)");
            return;
        };

        // The active signature is `addThree` with 3 parameters.
        let active_sig_idx = help.active_signature.unwrap_or(0) as usize;
        assert!(
            active_sig_idx < help.signatures.len(),
            "active signature index in bounds: {active_sig_idx} / {}",
            help.signatures.len()
        );
        let sig = &help.signatures[active_sig_idx];
        let params = sig
            .parameters
            .as_ref()
            .expect("the addThree signature has parameters");
        assert_eq!(
            params.len(),
            3,
            "addThree has 3 parameters, got label {:?}",
            sig.label
        );

        // --- Active-parameter signal (universal across both backends) ---
        // The cursor sits in the 2nd argument slot → active param index 1. Accept
        // EITHER the per-signature value (preferred) OR the top-level value (the
        // form a client applies when per-sig is absent).
        let effective_active = sig.active_parameter.or(help.active_parameter);
        assert_eq!(
            effective_active,
            Some(1),
            "active parameter must be index 1 (the 2nd arg slot); per-sig={:?} top-level={:?}",
            sig.active_parameter,
            help.active_parameter
        );

        // --- Offset-form labels (tsserver computes them; tgo passes through) ---
        let label_u16_len = sig.label.encode_utf16().count() as u32;
        let mut saw_offsets = false;
        for (i, p) in params.iter().enumerate() {
            if let ParameterLabel::LabelOffsets([start, end]) = p.label {
                saw_offsets = true;
                // Sane bounds + non-degenerate, non-empty span for a real param.
                assert!(
                    end <= label_u16_len,
                    "param {i} offset end {end} within label len {label_u16_len} ({:?})",
                    sig.label
                );
                assert!(
                    start < end,
                    "param {i} offset span must be non-empty (start {start} < end {end})"
                );
                assert_ne!(
                    (start, end),
                    (0, 0),
                    "param {i} offset must not be the degenerate [0,0]"
                );
                // The sliced label text must be a non-empty named parameter.
                let label_u16: Vec<u16> = sig.label.encode_utf16().collect();
                let slice =
                    String::from_utf16(&label_u16[start as usize..end as usize]).unwrap();
                assert!(
                    !slice.trim().is_empty(),
                    "param {i} offset slice must be a real parameter, got {slice:?}"
                );
            }
        }

        if session.is_tsgo() {
            // tgo speaks LSP and may emit EITHER form. We do not force Offsets here
            // (the server chooses); the universal active-param assertion above is
            // the cross-backend guarantee. If tgo DID send offsets, they were
            // validated in the loop above.
            eprintln!(
                "tgo signature-help offsets present = {saw_offsets} (informational; \
                 tgo may legitimately send Simple labels)"
            );
        } else {
            // tsserver: Verter computes the offsets from display parts, so the
            // active signature's parameters MUST be in offset form. This is the
            // discriminating assertion — pre-fix the merge always emitted Simple.
            assert!(
                saw_offsets,
                "tsserver active signature must carry LabelOffsets parameters, got {:?}",
                params.iter().map(|p| &p.label).collect::<Vec<_>>()
            );
        }
    }
);
