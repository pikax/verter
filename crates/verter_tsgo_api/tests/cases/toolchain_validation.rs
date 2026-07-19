//! Process-level toolchain validation coverage.
//!
//! Two engines drive these tests:
//! - the FAKE engine (`src/fake_engine.rs`, exposed as the feature-gated
//!   `verter_tsgo_fake_engine` bin via `CARGO_BIN_EXE_verter_tsgo_fake_engine`)
//!   — a deterministic stdio JSON-RPC responder whose scenario rides its FILE
//!   NAME, so parallel tests never share mutable environment. The `apiok`
//!   scenario serves the FULL `--api` attach surface (minted pipe, integer
//!   snapshot handle, staged project), so POSITIVE API-smoke coverage runs
//!   hermetically against the fake; and
//! - the REAL engine from the worktree's `node_modules`, when present
//!   (live-gated; `VERTER_REQUIRE_TSGO=1` turns an absence into a hard
//!   failure, matching the other live suites).

use std::path::PathBuf;
use std::time::Duration;

use verter_tsgo_api::toolchain::discovery::{enumerate_candidates, resolve, ResolutionRequest};
use verter_tsgo_api::toolchain::policy::VersionPolicy;
use verter_tsgo_api::toolchain::validation::{
    CandidateValidator, Capability, ProcessValidator, RejectionReason,
};

use super::common::workspace_root;

const FAKE_ENGINE: &str = env!("CARGO_BIN_EXE_verter_tsgo_fake_engine");

/// Copy the fake engine to a scenario-named path (the scenario is selected by
/// the binary's file name). Copies are shared per process; the copy lands via
/// an atomic rename so a parallel test never executes a partially-written file.
fn fake_engine(scenario: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("verter-tsgo-fake-engines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fake engine dir");
    let name = if cfg!(windows) {
        format!("verter-tsgo-fake-{scenario}.exe")
    } else {
        format!("verter-tsgo-fake-{scenario}")
    };
    let target = dir.join(name);
    if !target.exists() {
        // Serialize concurrent copiers, then copy to a sibling temp name and
        // atomically rename into place (a plain copy is observable mid-write).
        static COPY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = COPY_LOCK.lock().unwrap();
        if !target.exists() {
            let tmp = dir.join(format!(".copying-{}", scenario));
            std::fs::copy(FAKE_ENGINE, &tmp).expect("copy the fake engine");
            // A leftover from a killed earlier run must not block the rename.
            let _ = std::fs::remove_file(&target);
            std::fs::rename(&tmp, &target).expect("rename the fake engine into place");
        }
    }
    target
}

fn production_validator() -> ProcessValidator {
    ProcessValidator::with_policy(VersionPolicy::production())
        .with_bounds(Duration::from_secs(5), Duration::from_secs(15))
}

// ── DISCRIMINATING: a fake engine whose probe AND serverInfo agree on a
//    supported stable version VALIDATES for the --lsp requirement. ───────────
#[tokio::test]
async fn lsp_requirement_accepts_a_working_engine() {
    let engine = fake_engine("ok");
    let validated = production_validator()
        .validate(&engine, Capability::Lsp)
        .await
        .expect("a working fake engine must validate");
    assert_eq!(validated.path, engine);
    assert_eq!(validated.version_string, "7.0.2");
    assert_eq!(
        validated.version,
        verter_tsgo_api::toolchain::policy::TsgoVersion::new(7, 0, 2)
    );
}

// ── DISCRIMINATING: out-of-range / prerelease probes are rejected at the
//    POLICY step (before any --lsp spawn), with the actionable reason. ────────
#[tokio::test]
async fn policy_rejects_unsupported_and_prerelease_versions() {
    let err = production_validator()
        .validate(&fake_engine("v710"), Capability::Lsp)
        .await
        .expect_err("7.1.0 must be rejected");
    match err {
        RejectionReason::PolicyRejected { version, .. } => assert_eq!(version, "7.1.0"),
        other => panic!("expected PolicyRejected, got {other:?}"),
    }

    let err = production_validator()
        .validate(&fake_engine("rc"), Capability::Lsp)
        .await
        .expect_err("an rc must be rejected in production");
    assert!(
        matches!(err, RejectionReason::PolicyRejected { .. }),
        "{err:?}"
    );

    let err = production_validator()
        .validate(&fake_engine("nightly"), Capability::Lsp)
        .await
        .expect_err("a nightly must be rejected in production");
    match err {
        RejectionReason::PolicyRejected { rejection, .. } => {
            assert!(rejection
                .to_string()
                .contains("VERTER_TSGO_DEV_ALLOW_NIGHTLY"))
        }
        other => panic!("expected PolicyRejected, got {other:?}"),
    }
}

// ── DISCRIMINATING: the DEV-ONLY override admits the integer-handle nightly
//    end-to-end (policy + handshake + serverInfo agreement). ─────────────────
#[tokio::test]
async fn dev_override_admits_a_nightly_end_to_end() {
    let validator = ProcessValidator::with_policy(VersionPolicy::with_dev_nightly_override())
        .with_bounds(Duration::from_secs(5), Duration::from_secs(15));
    let validated = validator
        .validate(&fake_engine("nightly"), Capability::Lsp)
        .await
        .expect("the dev override must admit the integer-handle nightly");
    assert_eq!(validated.version_string, "7.0.0-dev.20260703.1");
}

// ── DISCRIMINATING: a serverInfo that disagrees with the probe is rejected —
//    the version-lie rail. ────────────────────────────────────────────────────
#[tokio::test]
async fn server_info_mismatch_is_rejected() {
    let err = production_validator()
        .validate(&fake_engine("mismatch"), Capability::Lsp)
        .await
        .expect_err("a probe/serverInfo disagreement must be rejected");
    match err {
        RejectionReason::ServerInfoVersionMismatch { probe, server_info } => {
            assert_eq!(probe, "7.0.2");
            assert_eq!(server_info, "7.0.9");
        }
        other => panic!("expected ServerInfoVersionMismatch, got {other:?}"),
    }
}

// ── DISCRIMINATING: an initialize result WITHOUT serverInfo fails the
//    handshake (the gate cannot observe a version). ───────────────────────────
#[tokio::test]
async fn missing_server_info_fails_the_handshake() {
    let err = production_validator()
        .validate(&fake_engine("noserverinfo"), Capability::Lsp)
        .await
        .expect_err("a missing serverInfo must fail the handshake");
    assert!(
        matches!(err, RejectionReason::LspHandshakeFailed { .. }),
        "{err:?}"
    );
}

// ── DISCRIMINATING: an engine that dies on `--lsp` fails the handshake. ──────
#[tokio::test]
async fn an_engine_exiting_on_lsp_fails_the_handshake() {
    let err = production_validator()
        .validate(&fake_engine("exit"), Capability::Lsp)
        .await
        .expect_err("an exiting engine must fail the handshake");
    assert!(
        matches!(err, RejectionReason::LspHandshakeFailed { .. }),
        "{err:?}"
    );
}

// ── DISCRIMINATING: the --api requirement goes BEYOND the handshake — a fake
//    whose pipe is dead fails at the --api smoke, not at version or LSP. ──────
#[tokio::test]
async fn api_requirement_fails_when_the_api_surface_is_dead() {
    let err = production_validator()
        .validate(&fake_engine("ok"), Capability::Api)
        .await
        .expect_err("a dead --api surface must fail the --api smoke");
    assert!(
        matches!(err, RejectionReason::ApiSmokeFailed { .. }),
        "{err:?}"
    );
}

// ── DISCRIMINATING (POSITIVE): a fake engine with a WORKING --api attach
//    surface (LSP handshake + minted pipe + initialize + integer snapshot
//    handle + the staged configured project) PASSES the full --api smoke.
//    Positive API coverage no longer depends on a real engine or silently
//    skips. ───────────────────────────────────────────────────────────────────
#[tokio::test]
async fn api_requirement_accepts_a_working_api_surface() {
    let validated = production_validator()
        .validate(&fake_engine("apiok"), Capability::Api)
        .await
        .expect("a fake engine serving the full --api attach surface must validate");
    assert_eq!(validated.version_string, "7.0.2");
}

// ── DISCRIMINATING (POSITIVE, LSP): the API-capable fake also passes the
//    cheaper --lsp requirement (its handshake + serverInfo are intact). ────────
#[tokio::test]
async fn lsp_requirement_accepts_the_api_capable_fake() {
    production_validator()
        .validate(&fake_engine("apiok"), Capability::Lsp)
        .await
        .expect("the API-capable fake must also validate for --lsp");
}

// ── live, real-engine coverage ───────────────────────────────────────────────

/// The real engine from the worktree (project-local tier only — deterministic:
/// no env override, no PATH, no cache, no bundled). `None` when the worktree
/// has no node_modules engine (a source-only CI lane skips; VERTER_REQUIRE_TSGO
/// makes that a hard failure like the other live suites).
fn real_engine_path() -> Option<PathBuf> {
    let root = workspace_root();
    let request = ResolutionRequest {
        requirement: Capability::Lsp,
        project_root: Some(root),
        env_override: None,
        path_entries: vec![],
        cache_root: None,
        host_exe: None,
    };
    let enumeration = enumerate_candidates(&request);
    let path = enumeration.candidates.into_iter().next().map(|c| c.path);
    if path.is_none() && std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
        panic!("VERTER_REQUIRE_TSGO is set but no project-local tsgo engine was enumerated");
    }
    path
}

// ── DISCRIMINATING (live): the real engine validates for BOTH requirements —
//    version probe, policy, LSP handshake + serverInfo agreement, and the full
//    --api attach + minimal-project snapshot with an integer handle. ──────────
#[tokio::test]
async fn real_engine_validates_for_lsp_and_api() {
    let Some(engine) = real_engine_path() else {
        eprintln!("[skip] no project-local tsgo engine in this worktree");
        return;
    };
    let validator = production_validator();
    let lsp = validator
        .validate(&engine, Capability::Lsp)
        .await
        .expect("the real engine must validate for --lsp");
    assert!(
        VersionPolicy::production().check(&lsp.version).is_ok(),
        "the real engine version must satisfy the production policy: {}",
        lsp.version_string
    );
    validator
        .validate(&engine, Capability::Api)
        .await
        .expect("the real engine must validate for --api");
}

// ── DISCRIMINATING (live): the production resolver end-to-end — enumeration +
//    validation + first-working selection over the real environment seams. ────
#[tokio::test]
async fn resolve_end_to_end_over_the_real_worktree() {
    let root = workspace_root();
    let request = ResolutionRequest {
        requirement: Capability::Lsp,
        project_root: Some(root),
        env_override: None,
        path_entries: vec![],
        cache_root: None,
        host_exe: None,
    };
    match resolve(&request).await {
        Ok(resolution) => {
            assert_eq!(
                resolution.provenance,
                verter_tsgo_api::toolchain::discovery::Provenance::ProjectLocal
            );
            assert!(resolution.path.is_file());
            assert!(
                VersionPolicy::production()
                    .check(&resolution.version)
                    .is_ok(),
                "resolved version must be supported: {}",
                resolution.version
            );
        }
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!("VERTER_REQUIRE_TSGO is set but resolution failed: {e}");
            }
            eprintln!("[skip] no usable tsgo engine in this worktree: {e}");
        }
    }
}
