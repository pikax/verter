//! Unit tests for the `verter_lsp` binary entry (CLI attestation pairing,
//! editor-tsserver topology, and the configured-project spawn-admission gate
//! with its canary engine). Split out of `main.rs` so the production entry
//! stays final-state source.

use std::fs;

use super::*;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn cli_pairs_editor_tsserver_receipt_with_its_nonce() {
    let args = CliArgs::parse_from([
        "--type-provider=auto".to_string(),
        "--editor-tsserver-receipt=C:/tmp/receipt.json".to_string(),
        format!("--editor-tsserver-nonce={NONCE}"),
        "C:/workspace".to_string(),
    ]);

    assert_eq!(
        args.editor_tsserver_receipt.as_deref(),
        Some("C:/tmp/receipt.json")
    );
    assert_eq!(args.editor_tsserver_nonce.as_deref(), Some(NONCE));
    assert_eq!(args.workspace_root.as_deref(), Some("C:/workspace"));
}

#[test]
fn cli_attestation_is_fail_closed_for_partial_or_stale_facts() {
    let partial = CliArgs::parse_from([format!("--editor-tsserver-nonce={NONCE}")]);
    assert!(partial.editor_tsserver_attestation().is_err());

    let file = tempfile::NamedTempFile::new().expect("temp receipt");
    fs::write(
        file.path(),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "nonce": "ffffffffffffffffffffffffffffffff",
            "pid": 4242,
            "projects": ["C:/workspace/tsconfig.json"]
        }))
        .expect("receipt json"),
    )
    .expect("write receipt");
    let args = CliArgs::parse_from([
        format!("--editor-tsserver-receipt={}", file.path().display()),
        format!("--editor-tsserver-nonce={NONCE}"),
    ]);
    assert!(args.editor_tsserver_attestation().is_err());
}

#[test]
fn editor_tsserver_topology_owns_no_semantic_child() {
    let topology =
        editor_tsserver_topology(&verter_lsp::editor_tsserver::EditorTsserverAttestation {
            pid: 4242,
            projects: vec!["C:/workspace/tsconfig.json".into()],
        });

    assert!(topology.0.is_none());
    assert_eq!(topology.1, TypeProviderKind::EditorTsserver);
    assert!(!topology.2);
    assert!(topology
        .3
        .as_deref()
        .is_some_and(|reason| reason.contains("4242")));
}

// ── DISCRIMINATING (H9): the configured-project admission gate runs BEFORE
//    any candidate spawn/smoke. A config-less workspace must perform ZERO
//    candidate spawns (a past regression had the resolver's probes
//    run before the config check). The canary "engine" logs every
//    invocation, so any spawn is observable. ──────────────────────────────

/// Plant a canary engine (a sh script logging its invocations) as the
/// project-local platform package; returns the workspace root and the log.
#[cfg(unix)]
fn plant_canary_engine(
    with_tsconfig: bool,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::TempDir::new().expect("temp workspace");
    let root = temp.path().join("workspace");
    let log = temp.path().join("spawns.log");
    let host = verter_tsgo_api::toolchain::platform::host_platform()
        .expect("test host is a supported platform");
    let pkg_lib = root
        .join("node_modules")
        .join(host.package_rel_path())
        .join("lib");
    fs::create_dir_all(&pkg_lib).expect("create package dirs");
    let canary = pkg_lib.join(host.executable);
    fs::write(
        &canary,
        format!(
            "#!/bin/sh\necho \"invoked: $*\" >> \"{}\"\nexit 1\n",
            log.display()
        ),
    )
    .expect("write canary");
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&canary).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&canary, perms).unwrap();
    }
    if with_tsconfig {
        fs::write(root.join("tsconfig.json"), "{}").expect("write tsconfig");
    }
    (temp, root, log)
}

#[cfg(unix)]
#[tokio::test]
async fn configless_workspace_performs_zero_candidate_spawns() {
    let (_temp, root, log) = plant_canary_engine(false);
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());
    let result = try_spawn_tsgo(&root.to_string_lossy(), &client_cell).await;
    let err = match result {
        Ok(_) => panic!("a config-less workspace must fail closed"),
        Err(err) => err,
    };
    assert!(
        err.contains("configured TypeScript project"),
        "the failure must name the configured-project precondition: {err}"
    );
    assert!(
        !log.exists(),
        "ZERO candidate spawns: the configured-project admission gate must run \
         BEFORE the resolver spawns/smokes any candidate, but the canary ran: {}",
        fs::read_to_string(&log).unwrap_or_default()
    );
}

// ── CONTROL (H9): a CONFIGURED workspace passes the admission gate, and
//    only then does the resolver spawn candidates (the canary runs). ──────
#[cfg(unix)]
#[tokio::test]
async fn configured_workspace_admits_then_spawns() {
    let (_temp, root, log) = plant_canary_engine(true);
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());
    // The canary exits 1 on every invocation, so resolution ultimately
    // fails — but the spawn must have HAPPENED (after admission).
    let _ = try_spawn_tsgo(&root.to_string_lossy(), &client_cell).await;
    assert!(
        log.exists(),
        "a configured workspace passes admission and the resolver then spawns candidates"
    );
}
