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

        // Place the cursor at the START of the `SIG_CURSOR` filler — i.e. the
        // 2nd-argument slot, immediately after the first comma — so the active
        // parameter is index 1 (`beta`). The marker is NOT stripped; it is harmless
        // filler occupying the 2nd-arg position, and tsserver/tgo resolve the active
        // param from the comma count BEFORE the cursor, which is unaffected by the
        // identifier sitting at/after the cursor.
        let marker_pos = session.find_position(&uri, "SIG_CURSOR", 0);
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

        // The fixture's parameter names are fixed, so the offset slices can be
        // compared EXACTLY against the known param texts.
        const EXPECTED_PARAMS: [&str; 3] = ["alpha: number", "beta: string", "gamma: boolean"];
        // The cursor sits in the 2nd argument slot → active param index 1 (`beta`).
        const EXPECTED_ACTIVE: u32 = 1;

        let label_u16: Vec<u16> = sig.label.encode_utf16().collect();
        let label_u16_len = label_u16.len() as u32;

        // Per-backend asymmetry: tsserver is OUR computed path — Verter assembles
        // the label and the per-param UTF-16 offset spans from tsserver's display
        // parts and stamps the per-signature active parameter, so we assert the
        // FULL fidelity (every param in offset form, each slice exact, per-sig
        // active param stamped directly). tgo speaks raw LSP and passes the
        // server's chosen form through (it may send Simple labels and may omit the
        // per-signature activeParameter), so there we assert only the cross-backend
        // active-param signal and validate offsets opportunistically when present.
        if !session.is_tsgo() {
            // --- tsserver: every parameter MUST be in offset form ---
            // Pre-fix the merge always emitted `ParameterLabel::Simple`, so this
            // EVERY-param offset assertion fails on the pre-K2 tree.
            for (i, p) in params.iter().enumerate() {
                let ParameterLabel::LabelOffsets([start, end]) = p.label else {
                    panic!(
                        "tsserver param {i} must be LabelOffsets (Verter computes offsets \
                         from display parts), got {:?}; all labels = {:?}",
                        p.label,
                        params.iter().map(|p| &p.label).collect::<Vec<_>>()
                    );
                };
                assert!(
                    start < end,
                    "tsserver param {i} offset span must be non-empty (start {start} < end {end})"
                );
                assert!(
                    end <= label_u16_len,
                    "tsserver param {i} offset end {end} within label len {label_u16_len} ({:?})",
                    sig.label
                );
                // The UTF-16 slice at [start, end) must equal the exact known param
                // text — proving the offsets index the right run of the label, not
                // merely a plausible-looking span.
                let slice = String::from_utf16(&label_u16[start as usize..end as usize]).unwrap();
                assert_eq!(
                    slice, EXPECTED_PARAMS[i],
                    "tsserver param {i} offset slice must equal the exact param text"
                );
            }

            // --- tsserver: per-signature active parameter stamped DIRECTLY ---
            // Do NOT fall back to the top-level value here — the per-signature stamp
            // on the selected overload is exactly the behavior under test. Pre-fix
            // the merge hard-coded `active_parameter: None`, so this fails on the
            // pre-K2 tree.
            assert_eq!(
                sig.active_parameter,
                Some(EXPECTED_ACTIVE),
                "tsserver must stamp the selected signature's per-sig active_parameter \
                 directly (the 2nd-arg slot, index 1)"
            );
        } else {
            // --- tgo: tolerant cross-backend active-param signal ---
            // tgo's top-level activeParameter is the real cross-backend signal;
            // accept the per-sig value when present, else the top-level value.
            let effective_active = sig.active_parameter.or(help.active_parameter);
            assert_eq!(
                effective_active,
                Some(EXPECTED_ACTIVE),
                "tgo active parameter must be index 1 (the 2nd arg slot); per-sig={:?} \
                 top-level={:?}",
                sig.active_parameter,
                help.active_parameter
            );

            // tgo may emit EITHER label form. If it DID send offsets, they must be
            // in-bounds, non-empty, and slice the exact param text (fail-closed in
            // the parser already rejects out-of-bounds/inverted spans).
            for (i, p) in params.iter().enumerate() {
                if let ParameterLabel::LabelOffsets([start, end]) = p.label {
                    assert!(
                        start < end && end <= label_u16_len,
                        "tgo param {i} offset span must be in-bounds and non-empty \
                         (start {start} end {end} len {label_u16_len})"
                    );
                    let slice =
                        String::from_utf16(&label_u16[start as usize..end as usize]).unwrap();
                    assert_eq!(
                        slice, EXPECTED_PARAMS[i],
                        "tgo param {i} offset slice (when present) must equal the exact param text"
                    );
                }
            }
        }
    }
);
