//! Guard: `in_band_witness_feeds_gate`.
//!
//! The attach substrate's engine-version witness is IN-BAND: the OWNED
//! handshake-half (`TsgoAttach::lsp_handshake` in
//! `crates/verter_tsgo_api/src/attach.rs`) reads `serverInfo.version` from the
//! LSP `initialize` result and feeds it to the fail-closed wire gate
//! (`gate::validate` over `ObservedEngine::from_in_band_server_info`); the
//! non-owning composer (`TsgoAttach::attach_to_initialized`) gates its
//! caller-supplied in-band version through the SAME gate per-attach; and the
//! accepted witness FLOWS to the `--api` `updateSnapshot` rail — the
//! `update_snapshot` convenience passes the stored `&self.observed_version`
//! into `update_snapshot_open_project`, never a hardcoded version literal.
//!
//! This STATIC guard reads `attach.rs`, brace-depth-extracts the three fn
//! bodies (mirroring `tsgo_capability_gate_on_version`), and asserts the
//! witness chain. The inline self-test proves the predicates DISCRIMINATE: a
//! handshake sample missing `gate::validate` /
//! `ObservedEngine::from_in_band_server_info` records failures, as does an
//! `update_snapshot` sample passing a hardcoded version instead of the stored
//! witness.

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

fn attach_rs() -> PathBuf {
    workspace_root().join("crates/verter_tsgo_api/src/attach.rs")
}

fn read_attach() -> String {
    let path = attach_rs();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Body of the fn whose signature starts with `sig`, extracted by a
/// brace-depth scan (find `sig`, advance to its opening `{`, take to the
/// matching `}`). `None` if absent or unbalanced.
fn extract_fn_body(src: &str, sig: &str) -> Option<String> {
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

/// Predicate 1: the OWNED handshake reads the in-band `serverInfo.version`
/// witness AND feeds it to the wire gate. Returns failures (empty ⇒ pass).
fn handshake_witness_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !body.contains("serverInfo") {
        failures.push(
            "lsp_handshake must read the in-band `serverInfo` from the \
             initialize result"
                .to_string(),
        );
    }
    if !body.contains("version") {
        failures.push(
            "lsp_handshake must read the `version` field of the in-band \
             serverInfo"
                .to_string(),
        );
    }
    if !body.contains("gate::validate") {
        failures.push(
            "lsp_handshake must gate the observed engine via `gate::validate` \
             (fail-closed)"
                .to_string(),
        );
    }
    if !body.contains("ObservedEngine::from_in_band_server_info") {
        failures.push(
            "lsp_handshake must construct the observation with the IN-BAND \
             witness (`ObservedEngine::from_in_band_server_info`), not a \
             --version probe"
                .to_string(),
        );
    }
    failures
}

/// Predicate 2: the non-owning composer gates its supplied in-band version
/// through the same gate, per-attach.
fn attach_to_initialized_gate_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !body.contains("gate::validate(&ObservedEngine::from_in_band_server_info(") {
        failures.push(
            "attach_to_initialized must gate the caller-supplied in-band \
             version via \
             `gate::validate(&ObservedEngine::from_in_band_server_info(...))` \
             per-attach (fail-closed)"
                .to_string(),
        );
    }
    failures
}

/// Predicate 3: the stored witness flows to the `--api` updateSnapshot rail —
/// `update_snapshot` passes `&self.observed_version` into
/// `update_snapshot_open_project`.
fn update_snapshot_witness_failures(body: &str) -> Vec<String> {
    let mut failures = Vec::new();
    if !body.contains("update_snapshot_open_project") {
        failures
            .push("update_snapshot must delegate to `update_snapshot_open_project`".to_string());
    }
    if !body.contains("&self.observed_version") {
        failures.push(
            "update_snapshot must pass the STORED in-band witness \
             (`&self.observed_version`) to the updateSnapshot rail — never a \
             hardcoded version literal"
                .to_string(),
        );
    }
    failures
}

#[test]
fn in_band_witness_feeds_gate() {
    let src = read_attach();

    let handshake_body = extract_fn_body(&src, "pub async fn lsp_handshake").unwrap_or_else(|| {
        panic!(
            "could not extract the `pub async fn lsp_handshake` body from {} — \
                 the OWNED handshake-half must exist and be brace-balanced",
            attach_rs().display()
        )
    });
    let ati_body =
        extract_fn_body(&src, "pub async fn attach_to_initialized").unwrap_or_else(|| {
            panic!(
                "could not extract the `pub async fn attach_to_initialized` body \
                 from {} — the non-owning composer must exist",
                attach_rs().display()
            )
        });
    let update_snapshot_body = extract_fn_body(&src, "pub async fn update_snapshot(")
        .unwrap_or_else(|| {
            panic!(
                "could not extract the `pub async fn update_snapshot(` body from \
                 {} — the stored-witness convenience must exist",
                attach_rs().display()
            )
        });

    let mut failures = handshake_witness_failures(&handshake_body);
    failures.extend(attach_to_initialized_gate_failures(&ati_body));
    failures.extend(update_snapshot_witness_failures(&update_snapshot_body));

    assert!(
        failures.is_empty(),
        "the in-band witness chain (crates/verter_tsgo_api/src/attach.rs) is \
         broken:\n{}",
        failures.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on a violating sample and
/// PASSES on a conforming one — the guard is non-vacuous.
#[test]
fn in_band_witness_feeds_gate_self_test_discriminates() {
    // A conforming handshake: reads serverInfo.version, gates it in-band.
    let good_handshake = r#"
        let init = conn.request("initialize", init_params).await?;
        let version = init.get("serverInfo").and_then(|s| s.get("version")).and_then(|v| v.as_str())
            .ok_or_else(|| TsgoApiError::UnsupportedTsgoWire("no serverInfo.version".into()))?;
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(version))?;
        conn.notify("initialized", serde_json::json!({})).await?;
        Ok(clearance)
    "#;
    assert!(
        handshake_witness_failures(good_handshake).is_empty(),
        "a handshake that reads serverInfo.version and gates it must pass"
    );

    // A violating handshake: completes the LSP dance but NEVER gates.
    let ungated_handshake = r#"
        conn.request("initialize", init_params).await?;
        conn.notify("initialized", serde_json::json!({})).await?;
        Ok(())
    "#;
    let ungated = handshake_witness_failures(ungated_handshake);
    assert!(
        ungated.iter().any(|f| f.contains("gate::validate")),
        "a handshake without `gate::validate` must fail; got {ungated:?}"
    );
    assert!(
        ungated.iter().any(|f| f.contains("serverInfo")),
        "a handshake that never reads serverInfo must fail; got {ungated:?}"
    );

    // A violating handshake: gates, but with the WRONG witness constructor (a
    // probe, not the in-band report).
    let wrong_witness_handshake = r#"
        let init = conn.request("initialize", init_params).await?;
        let version = init.get("serverInfo").and_then(|s| s.get("version")).and_then(|v| v.as_str()).unwrap();
        let clearance = gate::validate(&ObservedEngine::from_codec_wire(version))?;
        conn.notify("initialized", serde_json::json!({})).await?;
    "#;
    assert!(
        handshake_witness_failures(wrong_witness_handshake)
            .iter()
            .any(|f| f.contains("from_in_band_server_info")),
        "a handshake gating through the probe witness must fail"
    );

    // The non-owning composer: gated sample passes, ungated fails.
    let good_ati = r#"
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(observed_version.into()))?;
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts(lsp, api, session, clearance))
    "#;
    assert!(attach_to_initialized_gate_failures(good_ati).is_empty());
    let ungated_ati = r#"
        let (session, api) = Self::attach_api_session(&lsp.conn).await?;
        Ok(Self::from_parts_ungated(lsp, api, session, observed_version.into()))
    "#;
    assert!(
        !attach_to_initialized_gate_failures(ungated_ati).is_empty(),
        "a non-owning composer that skips the per-attach gate must fail"
    );

    // The updateSnapshot rail: the stored witness passes; a hardcoded version
    // literal fails.
    let good_update = r#"
        self.api.update_snapshot_open_project(tsconfig_path, &self.observed_version).await
    "#;
    assert!(update_snapshot_witness_failures(good_update).is_empty());
    let hardcoded_update = r#"
        self.api.update_snapshot_open_project(tsconfig_path, "7.0.1-rc").await
    "#;
    assert!(
        update_snapshot_witness_failures(hardcoded_update)
            .iter()
            .any(|f| f.contains("&self.observed_version")),
        "an update_snapshot passing a hardcoded version must fail"
    );

    // The extractor's `update_snapshot(` signature match must NOT be satisfied
    // by a `update_snapshot_open_project` definition (paren-anchored).
    let sample = r#"
impl Api {
    pub async fn update_snapshot_open_project(&self, a: &str, b: &str) -> R {
        hardcoded("7.0.1-rc")
    }

    pub async fn update_snapshot(&self, tsconfig_path: &str) -> R {
        self.api.update_snapshot_open_project(tsconfig_path, &self.observed_version).await
    }
}
"#;
    let body = extract_fn_body(sample, "pub async fn update_snapshot(")
        .expect("extracts the convenience body");
    assert!(
        body.contains("&self.observed_version") && !body.contains("hardcoded"),
        "the paren-anchored signature must select the convenience fn, not the \
         open-project op"
    );
}
