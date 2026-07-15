use super::*;

fn tool_root_tsserver(tsdk: Option<&str>, expected: Option<&str>) -> ToolRoot {
    ToolRoot {
        tsserver_tsdk: tsdk.map(String::from),
        expected_tsserver_js: expected.map(String::from),
        tsserver_version: Some("5.7.2".to_string()),
        tsgo_bin: None,
    }
}

// ── path-match enforcement (the global-npm rejection proof) ────────────

#[test]
fn matching_tsserver_paths_are_accepted() {
    assert!(enforce_tsserver_path_match(
        "/repo/node_modules/typescript/lib/tsserver.js",
        "/repo/node_modules/typescript/lib/tsserver.js",
    )
    .is_ok());
}

#[test]
fn ambient_global_npm_tsserver_is_rejected() {
    // Discovery resolved a global-npm tsserver.js; expected is the pinned
    // repo tsdk. The mismatch must be refused — proof the bridge never
    // silently accepts an ambient global TypeScript.
    let err = enforce_tsserver_path_match(
        "/repo/node_modules/typescript/lib/tsserver.js",
        "/usr/local/lib/node_modules/typescript/lib/tsserver.js",
    )
    .unwrap_err();
    assert!(matches!(err, ProviderInitError::PathMismatch { .. }));
    assert_eq!(err.kind(), ErrorKind::BaselineToolRootMismatch);
}

// ── strict failures ────────────────────────────────────────────────────

#[test]
fn strict_missing_tsserver_tool_root_fields_fail() {
    let no_node = || None;
    let no_tsgo = || None;
    let no_ts = |_: &str, _: &str| None;

    let err = resolve_with(
        ProviderName::Tsserver,
        &tool_root_tsserver(None, None),
        "/ws",
        true,
        &no_node,
        &no_tsgo,
        &no_ts,
    )
    .unwrap_err();
    assert!(matches!(err, ProviderInitError::MissingToolRootField(_)));
    assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
}

#[test]
fn strict_missing_node_fails() {
    let no_node = || None;
    // tsserver fields present; node missing.
    let err = resolve_tsserver_with(
        &tool_root_tsserver(Some("/repo/tsdk"), Some("/repo/tsdk/tsserver.js")),
        "/ws",
        false, // skip the existence assert so we exercise the node gate
        &no_node,
        &|_, _| Some("/repo/tsdk/tsserver.js".to_string()),
    )
    .unwrap_err();
    assert_eq!(err, ProviderInitError::ToolNotFound("node"));
}

#[test]
fn strict_missing_tsgo_fails() {
    let some_node = || Some("/usr/bin/node".to_string());
    let no_tsgo = || None;
    let no_ts = |_: &str, _: &str| None;
    let err = resolve_with(
        ProviderName::Tsgo,
        &ToolRoot::default(),
        "/ws",
        true,
        &some_node,
        &no_tsgo,
        &no_ts,
    )
    .unwrap_err();
    // Strict CI with no pinned tsgoBin is a missing-tool-root field — the pin
    // is required, never discovered (mirrors strict tsserver pinning).
    assert_eq!(err, ProviderInitError::MissingToolRootField("tsgoBin"));
    assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
}

#[test]
fn strict_mismatched_expected_tsserver_is_rejected() {
    // Real temp tsdk exists, discovery returns a DIFFERENT existing path.
    let dir = tempfile::tempdir().unwrap();
    let expected = dir.path().join("tsserver.js");
    std::fs::write(&expected, "// tsserver").unwrap();
    let other = dir.path().join("other_tsserver.js");
    std::fs::write(&other, "// other").unwrap();

    let expected_s = expected.to_string_lossy().to_string();
    let other_s = other.to_string_lossy().to_string();

    let err = resolve_tsserver_with(
        &tool_root_tsserver(
            Some(dir.path().to_string_lossy().as_ref()),
            Some(&expected_s),
        ),
        "/ws",
        true,
        &|| Some("/usr/bin/node".to_string()),
        &|_, _| Some(other_s.clone()),
    )
    .unwrap_err();
    assert!(matches!(err, ProviderInitError::PathMismatch { .. }));
}

#[test]
fn strict_matching_tsserver_tool_root_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let expected = dir.path().join("tsserver.js");
    std::fs::write(&expected, "// tsserver").unwrap();
    let expected_s = expected.to_string_lossy().to_string();
    let disc = expected_s.clone();

    let (used, plan) = resolve_tsserver_with(
        &tool_root_tsserver(
            Some(dir.path().to_string_lossy().as_ref()),
            Some(&expected_s),
        ),
        "/ws",
        true,
        &|| Some("/usr/bin/node".to_string()),
        &|_, _| Some(disc.clone()),
    )
    .unwrap();
    assert_eq!(used, canonicalize_path(&expected_s));
    assert!(matches!(plan, SpawnPlan::Tsserver { .. }));
}

// ── non-strict skip-with-reason ──────────────────────────────────────────

#[test]
fn non_strict_missing_provider_skips_with_recorded_reason() {
    let no_node = || None;
    let no_tsgo = || None;
    let no_ts = |_: &str, _: &str| None;
    let res = resolve_with(
        ProviderName::Tsgo,
        &ToolRoot::default(),
        "/ws",
        false,
        &no_node,
        &no_tsgo,
        &no_ts,
    )
    .unwrap();
    match res {
        Resolution::Skipped { reason } => {
            assert!(
                reason.contains("tsgo"),
                "reason must record the tool: {reason}"
            );
            assert!(reason.contains("non-strict"), "reason: {reason}");
        }
        Resolution::Ready { .. } => panic!("expected skip in non-strict with no tsgo"),
    }
}

// ── strict tsgo pinning refuses the discovery fallback ───────────────────

#[test]
fn strict_tsgo_refuses_discovery_fallback_for_invalid_pinned_bin() {
    // A pinned tsgo path that does NOT exist must hard-error in strict mode,
    // never silently fall back to an ambient discovered tsgo (mirrors the
    // strict tsserver path-pinning).
    let tool_root = ToolRoot {
        tsgo_bin: Some("/nonexistent/pinned/tsgo".to_string()),
        ..ToolRoot::default()
    };
    let err = resolve_with(
        ProviderName::Tsgo,
        &tool_root,
        "/ws",
        true, // strict
        &|| Some("/usr/bin/node".to_string()),
        &|| Some("/somewhere/else/tsgo".to_string()), // discovery WOULD succeed
        &|_, _| None,
    )
    .unwrap_err();
    // Pinned-but-absent → ExpectedMissing, NOT a fallback to the ambient tsgo.
    assert!(
        matches!(err, ProviderInitError::ExpectedMissing(_)),
        "strict mode must refuse the discovery fallback for an invalid pinned tsgo; got {err:?}"
    );
    assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
}

#[test]
fn strict_tsgo_missing_pinned_field_refuses_discovery_fallback() {
    // No pinned tsgoBin at all in strict mode is a missing-tool-root field,
    // not a discovery fallback.
    let err = resolve_with(
        ProviderName::Tsgo,
        &ToolRoot::default(),
        "/ws",
        true, // strict
        &|| Some("/usr/bin/node".to_string()),
        &|| Some("/discovered/tsgo".to_string()), // discovery WOULD succeed
        &|_, _| None,
    )
    .unwrap_err();
    assert_eq!(
        err,
        ProviderInitError::MissingToolRootField("tsgoBin"),
        "strict mode with no pinned tsgoBin must be a missing-field error"
    );
    assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
}

#[test]
fn non_strict_tsgo_invalid_pinned_bin_falls_back_to_discovery() {
    // Non-strict keeps the lenient fallback: an invalid pinned bin still
    // discovers an ambient tsgo so local dev is not blocked.
    let tool_root = ToolRoot {
        tsgo_bin: Some("/nonexistent/pinned/tsgo".to_string()),
        ..ToolRoot::default()
    };
    let res = resolve_with(
        ProviderName::Tsgo,
        &tool_root,
        "/ws",
        false, // non-strict
        &|| Some("/usr/bin/node".to_string()),
        &|| Some("/discovered/tsgo".to_string()),
        &|_, _| None,
    )
    .unwrap();
    match res {
        Resolution::Ready { plan, .. } => {
            assert_eq!(
                plan,
                SpawnPlan::Tsgo {
                    bin: "/discovered/tsgo".to_string()
                }
            );
        }
        Resolution::Skipped { .. } => panic!("non-strict should discover a fallback tsgo"),
    }
}

#[test]
fn ready_tsgo_uses_explicit_existing_bin_over_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("tsgo");
    std::fs::write(&bin, "#!/bin/sh\n").unwrap();
    let bin_s = bin.to_string_lossy().to_string();

    let tool_root = ToolRoot {
        tsgo_bin: Some(bin_s.clone()),
        ..ToolRoot::default()
    };
    // Discovery would return something else, but the explicit existing bin wins.
    let res = resolve_with(
        ProviderName::Tsgo,
        &tool_root,
        "/ws",
        true,
        &|| None,
        &|| Some("/somewhere/else/tsgo".to_string()),
        &|_, _| None,
    )
    .unwrap();
    match res {
        Resolution::Ready { plan, .. } => {
            assert_eq!(plan, SpawnPlan::Tsgo { bin: bin_s });
        }
        Resolution::Skipped { .. } => panic!("explicit bin should be ready"),
    }
}
