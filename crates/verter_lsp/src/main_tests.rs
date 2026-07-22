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

// ── DISCRIMINATING: only the EXPLICIT `editor-tsserver` policy may adopt an
//    editor plugin attestation. The editor-owned tier leaves the LSP with no
//    engine of its own, so a policy that adopts it without the user asking can
//    silently hand a whole workspace to a plugin that cannot serve it.
#[test]
fn only_the_explicit_editor_tsserver_route_adopts_the_plugin_attestation() {
    assert!(route_consumes_editor_tsserver_attestation(
        "editor-tsserver"
    ));
    for route in [
        "auto",
        "tsserver",
        "shared-tsgo",
        "tsgo",
        "extension",
        "off",
    ] {
        assert!(
            !route_consumes_editor_tsserver_attestation(route),
            "route {route} must not adopt an editor tsserver attestation"
        );
    }
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
    // The status surface must NAME this topology, not just its engine family.
    assert_eq!(topology.2, TypeProviderTopology::EditorTsserver);
    assert_eq!(topology.2.wire(), "editor-tsserver");
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

/// A TS7-family workspace install (here a 7.0.1-rc) resolved by the
/// explicit `--type-provider=tsserver` route classifies as the native
/// (tsgo) family BEFORE any binary lookup or spawn — the typed
/// `NativeFamily` error is the reclassification signal the tsserver arm
/// turns into the managed-TSGO route.
#[tokio::test]
async fn tsserver_route_reclassifies_ts7_family_install_before_any_spawn() {
    let tmp = tempfile::tempdir().unwrap();
    let ts_dir = tmp.path().join("node_modules").join("typescript");
    fs::create_dir_all(ts_dir.join("lib")).unwrap();
    fs::write(ts_dir.join("lib").join("tsserver.js"), "// launcher").unwrap();
    fs::write(
        ts_dir.join("package.json"),
        r#"{ "name": "typescript", "version": "7.0.1-rc" }"#,
    )
    .unwrap();

    let args = CliArgs::parse_from(["--type-provider=tsserver".to_string()]);
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());
    match try_spawn_tsserver(&args, tmp.path().to_str().unwrap(), &client_cell).await {
        Ok(_) => panic!("a native-family install must never spawn as tsserver"),
        Err(err) => assert!(
            matches!(err, TsserverSpawnError::NativeFamily { major: 7 }),
            "expected NativeFamily {{ major: 7 }}, got {err:?}"
        ),
    }
}

// ── W5/FIX-4 + FIX-3: the managed fallback is CAPABILITY-driven, and it
//    never claims a provider it cannot obtain. `choose_managed_engine` is
//    the pure decision the IO probe feeds; every arm below is a state a
//    real workspace reaches. ──────────────────────────────────────────────

fn facts(
    has_configured_project: bool,
    tsgo_candidate: Option<&str>,
    tsserver: Option<&str>,
    node: Option<&str>,
) -> ManagedEngineFacts {
    ManagedEngineFacts {
        workspace_root: "C:/workspace".to_string(),
        has_configured_project,
        tsgo_candidate: tsgo_candidate.map(str::to_string),
        tsgo_notes: Vec::new(),
        tsserver: tsserver.map(str::to_string),
        node: node.map(str::to_string),
    }
}

// ── DISCRIMINATING: tsgo is PREFERRED whenever the project can supply one. ──
#[test]
fn managed_fallback_prefers_tsgo_when_a_candidate_exists() {
    let choice = choose_managed_engine(&facts(
        true,
        Some("C:/ws/node_modules/@typescript/typescript-win32-x64/lib/tsc.exe"),
        Some("C:/ws/node_modules/typescript/lib/tsserver.js"),
        Some("node"),
    ));
    match choice {
        ManagedEngineChoice::Tsgo { detail } => assert!(detail.contains("tsc.exe"), "{detail}"),
        other => panic!("tsgo must win when it is available: {other:?}"),
    }
}

// ── DISCRIMINATING (the reported symptom): a project pinned to TypeScript
//    5.x has NO tsgo anywhere but ships a perfectly good tsserver. Before
//    this fix the managed fallback was tsgo-only and such a project got NO
//    semantics at all. ─────────────────────────────────────────────────────
#[test]
fn managed_fallback_admits_tsserver_when_no_tsgo_exists() {
    let choice = choose_managed_engine(&facts(
        true,
        None,
        Some("C:/ws/node_modules/typescript/lib/tsserver.js"),
        Some("node"),
    ));
    match choice {
        ManagedEngineChoice::Tsserver { detail } => {
            assert!(detail.contains("tsserver.js"), "{detail}");
            assert!(
                detail.contains("no supported tsgo"),
                "the reason must state WHY tsserver was chosen: {detail}"
            );
        }
        other => panic!("tsserver is an accepted tier-2 fallback: {other:?}"),
    }
}

// ── DISCRIMINATING (FIX-3): nothing obtainable ⇒ None WITH a reason, never
//    a provider that would later report "connected" with no engine. ────────
#[test]
fn managed_fallback_reports_none_with_a_reason_when_no_engine_exists() {
    match choose_managed_engine(&facts(true, None, None, None)) {
        ManagedEngineChoice::None { reason } => {
            assert!(reason.contains("no TypeScript engine"), "{reason}");
            assert!(
                reason.contains("tsgo") && reason.contains("tsserver"),
                "the reason must name both engines that were searched: {reason}"
            );
        }
        other => panic!("no engine anywhere must report None: {other:?}"),
    }
}

// ── DISCRIMINATING (FIX-3): tsserver present but Node absent is still no
//    engine — the reason names Node, not a bare "not found". ───────────────
#[test]
fn managed_fallback_reports_none_when_tsserver_has_no_node() {
    match choose_managed_engine(&facts(
        true,
        None,
        Some("C:/ws/node_modules/typescript/lib/tsserver.js"),
        None,
    )) {
        ManagedEngineChoice::None { reason } => {
            assert!(reason.contains("Node.js"), "{reason}")
        }
        other => panic!("tsserver without node is not an engine: {other:?}"),
    }
}

// ── DISCRIMINATING (FIX-3, Corpus C shape): a workspace with ZERO
//    tsconfigs obtains NO managed engine — both engines are project-bound —
//    and the reason says so instead of claiming a connected provider. ──────
#[test]
fn managed_fallback_fails_closed_and_says_why_without_a_configured_project() {
    match choose_managed_engine(&facts(
        false,
        Some("C:/ws/node_modules/@typescript/typescript-win32-x64/lib/tsc.exe"),
        Some("C:/ws/node_modules/typescript/lib/tsserver.js"),
        Some("node"),
    )) {
        ManagedEngineChoice::None { reason } => {
            assert!(
                reason.contains("configured TypeScript project"),
                "the reason must name the missing precondition: {reason}"
            );
            assert!(reason.contains("C:/workspace"), "{reason}");
        }
        other => panic!("a config-less workspace must fail closed: {other:?}"),
    }
}

// ── DISCRIMINATING: resolver tier NOTES (a stale VERTER_TSGO_BIN, a
//    skipped untrusted cache) survive into the no-engine reason — the user
//    must be able to act on it. ────────────────────────────────────────────
#[test]
fn no_engine_reason_carries_the_resolver_tier_notes() {
    let mut f = facts(true, None, None, None);
    f.tsgo_notes = vec!["VERTER_TSGO_BIN points at C:/gone which is not a usable file".into()];
    match choose_managed_engine(&f) {
        ManagedEngineChoice::None { reason } => {
            assert!(reason.contains("VERTER_TSGO_BIN"), "{reason}")
        }
        other => panic!("expected None, got {other:?}"),
    }
}

// ── DISCRIMINATING (W5/FIX-4): candidate ENUMERATION is existence-only, so a
//    TypeScript 5.x workspace contributes its `node_modules/.bin/tsc.cmd`
//    shim. Counting that as an available tsgo sends the session down the tsgo
//    route, where it dies at the version gate — the exact "no engine at all"
//    outcome the tsserver fallback exists to prevent. Observed live against a
//    real TS 5.3.3 project before this filter existed. ──────────────────────
#[test]
fn a_ts5_bin_shim_is_not_a_plausible_tsgo_candidate() {
    use verter_tsgo_api::toolchain::discovery::Provenance;
    let shim = std::path::Path::new("C:/ws/node_modules/.bin/tsc.cmd");
    assert!(
        !plausible_tsgo_candidate(shim, Provenance::ProjectLocal, false),
        "a .bin shim in a non-native-family workspace must not count as tsgo"
    );
    // The same shim IS plausible when the workspace install is the TS7 family.
    assert!(plausible_tsgo_candidate(
        shim,
        Provenance::ProjectLocal,
        true
    ));
}

// ── CONTROL: the genuine tsgo shapes stay plausible — the `@typescript`
//    platform package (which only tsgo publishes) and every non-project-local
//    tier, whose real validator decides. ────────────────────────────────────
#[test]
fn genuine_tsgo_shapes_stay_plausible_candidates() {
    use verter_tsgo_api::toolchain::discovery::Provenance;
    assert!(plausible_tsgo_candidate(
        std::path::Path::new("C:/ws/node_modules/@typescript/typescript-win32-x64/lib/tsc.exe"),
        Provenance::ProjectLocal,
        false,
    ));
    for provenance in [
        Provenance::EnvOverride,
        Provenance::SharedPath,
        Provenance::TempCache,
        Provenance::Bundled,
    ] {
        assert!(
            plausible_tsgo_candidate(
                std::path::Path::new("C:/anywhere/tsc.exe"),
                provenance,
                false
            ),
            "{provenance:?} is operator- or policy-controlled and stays plausible"
        );
    }
}

// ── DISCRIMINATING: the two tsgo TOPOLOGIES must be distinguishable from the
//    status surface alone. Both report the `tsgo` FAMILY, so a status that
//    carries only the family cannot tell a user whether Verter attached to the
//    engine their editor was already running or spawned a second one — which is
//    exactly how a serving shared tier was reported as a routing failure.
#[test]
fn the_two_tsgo_topologies_are_distinguishable_on_the_wire() {
    assert_eq!(TypeProviderTopology::SharedTsgo.wire(), "shared-tsgo");
    assert_eq!(TypeProviderTopology::ManagedTsgo.wire(), "managed-tsgo");
    assert_ne!(
        TypeProviderTopology::SharedTsgo.wire(),
        TypeProviderTopology::ManagedTsgo.wire()
    );
    // Both are the same engine family, which is why the family cannot report it.
    assert_eq!(
        TypeProviderTopology::implied_by(TypeProviderKind::Tsgo),
        TypeProviderTopology::ManagedTsgo,
        "an editor-owned attach is never IMPLIED — it is chosen explicitly"
    );
    for topology in [
        TypeProviderTopology::SharedTsgo,
        TypeProviderTopology::ManagedTsgo,
        TypeProviderTopology::WorkspaceTsserver,
        TypeProviderTopology::EditorTsserver,
        TypeProviderTopology::ExtensionHosted,
        TypeProviderTopology::None,
    ] {
        assert!(!topology.wire().is_empty());
    }
}
