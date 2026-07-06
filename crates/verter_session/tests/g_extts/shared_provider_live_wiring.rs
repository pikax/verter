//! Guard: `shared_provider_live_wiring`.
//!
//! The existing SHARED-mode g_extts guards
//! (`tsgo_shared_mode_carrier_injection`, `shared_mode_requires_full_ts_lsp_proxy`,
//! `shared_mode_no_unmapped_carrier_path_leak`,
//! `shared_mode_failover_is_per_reference_closure`) pin the SHARED substrate SHAPE
//! in `verter_tsgo_api` / `verter_session`. This guard extends that coverage to the
//! LIVE PRODUCTION CONSUMER — `verter_lsp`'s `TsgoSharedProvider` — so the shared
//! substrate is not just a shape with no wired serve path (the zero-call-site
//! anti-pattern). `verter_session` does not link `verter_lsp`, so — like the sibling
//! guards that scan `verter_tsgo_api` source — this asserts the SOURCE STRUCTURE of
//! `crates/verter_lsp/src/tsgo/shared.rs` + `main.rs`; the BEHAVIORAL proofs live in
//! `crates/verter_lsp/tests/shared_provider_live.rs` (the live macro case +
//! carrier-leak + split-brain negatives against the real engine) and the
//! `tsgo::shared` unit tests (the fail-open / URI-identity discriminators).
//!
//! Six structural facts:
//!
//!   1. Mode is decided through the ONE shared live decision layer:
//!      `TsgoSharedProvider::establish_shared` routes through `decide_shared_serve`,
//!      which composes the five provenance facts and calls `decide_live` — never a
//!      private per-provider mode heuristic.
//!   2. SHARED requires ALL FIVE provenance-typed eligibility facts —
//!      `VersionGateFact`, `AttachFact`, `BindingFact`, `ProxyFact`,
//!      `EditorBindingFact` — assembled into `EligibilityFacts`.
//!   3. A non-SHARED decision fails CLOSED: `establish_shared` returns
//!      `EstablishError::NotShared` (and detaches) when the decision mode is not
//!      SHARED — SHARED is never fabricated.
//!   4. Carrier injection goes through the shim's CONTROL channel
//!      (`carrier_did_open_synced` / `carrier_did_change_synced` /
//!      `carrier_did_close` on the `ControlClient`), NOT an OWNED `--lsp` didOpen.
//!   5. Diagnostic map-back reuses the ONE shared authority
//!      `position_carrier_diagnostics` — no forged `(0,0)` span, no second mapper.
//!   6. `main.rs` wires `try_attach_shared_tsgo` as a fail-closed sibling of
//!      `try_spawn_tsgo`: SHARED is attempted only behind the rendezvous evidence
//!      and falls through to the OWNED baseline on any error.
//!
//! The inline self-test proves the predicates DISCRIMINATE.

use std::fs;
use std::path::PathBuf;

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_source(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The body of the block whose header starts with `sig`, by brace-depth scan.
fn block_body(src: &str, sig: &str) -> Option<String> {
    let sig_at = src.find(sig)?;
    let after_sig = &src[sig_at..];
    let open_rel = after_sig.find('{')?;
    let bytes = after_sig.as_bytes();
    let mut depth = 0usize;
    let mut i = open_rel;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after_sig[open_rel + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Fact 1: the decision routes through the shared live layer.
fn decision_layer_failures(shared_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !shared_src.contains("fn decide_shared_serve") {
        failures.push(
            "shared.rs must expose `decide_shared_serve` — the single SHARED mode oracle"
                .to_string(),
        );
    }
    if !shared_src.contains("decide_live(") {
        failures.push(
            "`decide_shared_serve` must call the shared `decide_live` — no private per-provider \
             mode heuristic"
                .to_string(),
        );
    }
    if !shared_src.contains("compose_eligibility(") {
        failures.push(
            "`decide_shared_serve` must compose eligibility through `compose_eligibility`"
                .to_string(),
        );
    }
    failures
}

/// Fact 2: the five provenance facts + their aggregate.
fn provenance_fact_failures(shared_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for fact in [
        "VersionGateFact",
        "AttachFact",
        "BindingFact",
        "ProxyFact",
        "EditorBindingFact",
        "EligibilityFacts",
    ] {
        if !shared_src.contains(fact) {
            failures.push(format!(
                "shared.rs must consume the provenance-typed `{fact}` — SHARED requires \
                 all-positive typed evidence"
            ));
        }
    }
    failures
}

/// Fact 3: a non-SHARED decision fails closed.
fn fail_closed_failures(establish_body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !establish_body.contains("ServeMode::Shared") {
        failures.push(
            "`establish_shared` must gate on `ServeMode::Shared` before returning a provider"
                .to_string(),
        );
    }
    if !establish_body.contains("EstablishError::NotShared") {
        failures.push(
            "`establish_shared` must return `EstablishError::NotShared` when the decision is not \
             SHARED (fail closed to the OWNED baseline — never fabricate SHARED)"
                .to_string(),
        );
    }
    failures
}

/// Fact 4: carrier injection is control-channel-only.
fn injection_failures(shared_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !shared_src.contains("carrier_did_open_synced") {
        failures.push(
            "shared.rs must inject carriers through the shim CONTROL channel \
             (`carrier_did_open_synced`), NOT an OWNED `--lsp` didOpen"
                .to_string(),
        );
    }
    if !shared_src.contains("carrier_did_close") {
        failures.push(
            "shared.rs must retract carriers through the control channel (`carrier_did_close`)"
                .to_string(),
        );
    }
    failures
}

/// Fact 5: map-back uses the ONE shared authority.
fn mapback_failures(shared_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !shared_src.contains("position_carrier_diagnostics") {
        failures.push(
            "shared.rs must map `--api` diagnostics through the ONE shared authority \
             `position_carrier_diagnostics` — no forged (0,0), no second mapper"
                .to_string(),
        );
    }
    failures
}

/// Fact 6: main.rs wires the fail-closed sibling.
fn main_wiring_failures(main_src: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !main_src.contains("fn try_attach_shared_tsgo") {
        failures.push(
            "main.rs must carry `try_attach_shared_tsgo` — the SHARED sibling of `try_spawn_tsgo`"
                .to_string(),
        );
    }
    if !main_src.contains("fn try_spawn_tsgo") {
        failures.push(
            "main.rs must still carry the OWNED `try_spawn_tsgo` baseline (the fall-through target)"
                .to_string(),
        );
    }
    // SHARED must be gated behind the opt-in rendezvous evidence (never the default).
    if !main_src.contains("shared_rendezvous") {
        failures.push(
            "main.rs must gate the SHARED attempt behind `shared_rendezvous` (opt-in evidence) — \
             SHARED is never the default when the rendezvous is absent"
                .to_string(),
        );
    }
    failures
}

#[test]
fn shared_provider_live_wiring() {
    let shared_src = read_source("crates/verter_lsp/src/tsgo/shared.rs");
    let main_src = read_source("crates/verter_lsp/src/main.rs");
    let establish_body = block_body(&shared_src, "pub async fn establish_shared")
        .expect("shared.rs must carry `establish_shared`");

    let mut failures = decision_layer_failures(&shared_src);
    failures.extend(provenance_fact_failures(&shared_src));
    failures.extend(fail_closed_failures(&establish_body));
    failures.extend(injection_failures(&shared_src));
    failures.extend(mapback_failures(&shared_src));
    failures.extend(main_wiring_failures(&main_src));

    assert!(
        failures.is_empty(),
        "the SHARED editor-attach provider live wiring \
         (crates/verter_lsp/src/tsgo/shared.rs + main.rs) is violated:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and PASSES
/// on a conforming one.
#[test]
fn shared_provider_live_wiring_self_test_discriminates() {
    // Fact 1.
    let good = "fn decide_shared_serve(...) { let _ = compose_eligibility(&facts); decide_live(&request, ...) }";
    assert!(decision_layer_failures(good).is_empty());
    assert!(
        decision_layer_failures("fn pick_mode() { ServeMode::Shared }")
            .iter()
            .any(|f| f.contains("decide_shared_serve") || f.contains("decide_live")),
        "a provider that decides mode without the shared layer must fail"
    );

    // Fact 2.
    let good_facts =
        "VersionGateFact AttachFact BindingFact ProxyFact EditorBindingFact EligibilityFacts";
    assert!(provenance_fact_failures(good_facts).is_empty());
    assert!(
        provenance_fact_failures("AttachFact BindingFact")
            .iter()
            .any(|f| f.contains("VersionGateFact")),
        "a provider missing a provenance fact must fail"
    );

    // Fact 3.
    let good_body =
        "if decision.mode() != ServeMode::Shared { return Err(EstablishError::NotShared(d)); }";
    assert!(fail_closed_failures(good_body).is_empty());
    assert!(
        fail_closed_failures("Ok(Self { .. })")
            .iter()
            .any(|f| f.contains("NotShared")),
        "an establish body that never fails closed on a non-SHARED decision must fail"
    );

    // Fact 4.
    let good_inject = "self.control.carrier_did_open_synced(..).await?; self.control.carrier_did_close(..).await?;";
    assert!(injection_failures(good_inject).is_empty());
    assert!(
        injection_failures("self.lsp.open_file(path, content).await")
            .iter()
            .any(|f| f.contains("carrier_did_open_synced")),
        "an OWNED --lsp didOpen injection must fail the control-channel fact"
    );

    // Fact 5.
    assert!(mapback_failures("position_carrier_diagnostics(&diags, content, &c)").is_empty());
    assert!(
        mapback_failures("TypeDiagnostic { start: 0, end: 0, .. }")
            .iter()
            .any(|f| f.contains("position_carrier_diagnostics")),
        "a forged-span map-back must fail"
    );

    // Fact 6.
    let good_main =
        "fn try_attach_shared_tsgo() {} fn try_spawn_tsgo() {} args.shared_rendezvous()";
    assert!(main_wiring_failures(good_main).is_empty());
    assert!(
        main_wiring_failures("fn try_spawn_tsgo() {}")
            .iter()
            .any(|f| f.contains("try_attach_shared_tsgo")),
        "a main without the SHARED sibling must fail"
    );
    assert!(
        main_wiring_failures("fn try_attach_shared_tsgo() {} fn try_spawn_tsgo() {}")
            .iter()
            .any(|f| f.contains("shared_rendezvous")),
        "a SHARED attempt not gated behind the opt-in rendezvous must fail"
    );
}
