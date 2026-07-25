use super::*;

/// A real (symlink-resolved) temp-directory root.
///
/// `tempfile::tempdir()` hands back the platform temp path AS SPELLED, and that
/// spelling is not always the file's identity: on macOS `/var` is a symlink to
/// `/private/var`, and on a Linux distro where `/tmp` is a symlink the same
/// holds. Tool-root resolution publishes the path's filesystem IDENTITY, so a
/// fixture that compares a resolved output against the spelled temp path is
/// asserting a platform accident — it happens to hold wherever the temp root has
/// no symlink component and fails wherever it does. Resolving the root ONCE up
/// front makes every fixture path built below it already-real on macOS, Linux
/// and Windows alike, so the comparison is platform-neutral.
///
/// Fixtures that deliberately exercise a symlinked spelling build the link
/// explicitly below this real root.
fn real_temp_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
    std::fs::canonicalize(dir.path()).expect("temp root must resolve")
}

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

// ── boundary normalisation (external path -> the one internal form) ────

/// The pnpm-symlink case that failed EVERY Linux CI run: one tsserver.js
/// reached both by its symlinked spelling and by its real path is the SAME
/// file, so boundary normalisation must map both to one internal value.
/// String canonicalisation alone cannot reconcile them, so before this fix the
/// two spellings reached the comparison unequal and it reported
/// `baseline_tool_root_mismatch`.
///
/// Unix-gated because it needs a real symlink: the failure is a POSIX
/// condition, and creating symlinks on Windows is privileged.
#[cfg(unix)]
#[test]
fn boundary_normalisation_collapses_a_symlinked_spelling_to_one_value() {
    let fs = NativeFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let store_ts = tmp.path().join("store").join("typescript");
    let real_lib = store_ts.join("lib");
    std::fs::create_dir_all(&real_lib).unwrap();
    let real_js = real_lib.join("tsserver.js");
    std::fs::write(&real_js, "// tsserver").unwrap();

    // `<pkg>/node_modules/typescript` -> `<store>/typescript`: the pnpm layout.
    let link_parent = tmp.path().join("pkg").join("node_modules");
    std::fs::create_dir_all(&link_parent).unwrap();
    let link = link_parent.join("typescript");
    std::os::unix::fs::symlink(&store_ts, &link).unwrap();

    let spelled = normalize_tool_path(&fs, &link.join("lib").join("tsserver.js").to_string_lossy());
    let real = normalize_tool_path(&fs, &real_js.to_string_lossy());
    assert_eq!(
        spelled, real,
        "two spellings of one tsserver.js must normalise to a single internal value",
    );
    assert!(enforce_tsserver_path_match(&spelled, &real).is_ok());
}

/// Normalisation must NOT weaken the gate: two tsserver.js files that really
/// are different files keep distinct internal values even when BOTH exist on
/// disk (where `fs::canonicalize` succeeds for each and cannot collapse them).
#[test]
fn boundary_normalisation_keeps_distinct_files_distinct() {
    let fs = NativeFs::new();
    let tmp = tempfile::tempdir().unwrap();
    let pinned = tmp.path().join("pinned");
    let ambient = tmp.path().join("ambient");
    std::fs::create_dir_all(&pinned).unwrap();
    std::fs::create_dir_all(&ambient).unwrap();
    let pinned_js = pinned.join("tsserver.js");
    let ambient_js = ambient.join("tsserver.js");
    std::fs::write(&pinned_js, "// pinned").unwrap();
    std::fs::write(&ambient_js, "// ambient").unwrap();

    let e = normalize_tool_path(&fs, &pinned_js.to_string_lossy());
    let d = normalize_tool_path(&fs, &ambient_js.to_string_lossy());
    assert_ne!(e, d, "distinct files must not normalise to one value");

    let err = enforce_tsserver_path_match(&e, &d).unwrap_err();
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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

/// A pinned tool root spelled by its REAL path resolves ready, and the
/// published `tool_root_used` is that path in the one internal form.
///
/// The fixture root is symlink-resolved up front (`real_temp_root`) so the
/// assertion holds on every platform: tool-root resolution publishes filesystem
/// identity, so comparing against an unresolved temp spelling would pass only
/// where the platform's temp root happens to have no symlink component.
#[test]
fn strict_matching_tsserver_tool_root_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = real_temp_root(&dir);
    let expected = root.join("tsserver.js");
    std::fs::write(&expected, "// tsserver").unwrap();
    let expected_s = expected.to_string_lossy().to_string();
    let disc = expected_s.clone();

    let (used, plan) = resolve_tsserver_with(
        &NativeFs::new(),
        &tool_root_tsserver(Some(root.to_string_lossy().as_ref()), Some(&expected_s)),
        "/ws",
        true,
        &|| Some("/usr/bin/node".to_string()),
        &|_, _| Some(disc.clone()),
    )
    .unwrap();
    assert_eq!(used, canonicalize_path(&expected_s));
    assert!(matches!(plan, SpawnPlan::Tsserver { .. }));
}

/// The symlinked-tsserver fix at the RESOLVE boundary: the harness pins the
/// tool root through the pnpm symlink spelling while discovery reports the
/// store's real path. Both name ONE file, so resolution must succeed — and it
/// must publish the file's real path, not the spelling it was asked with.
///
/// Unix-gated for the same reason as the normalisation-level twin: the failure
/// is a POSIX symlink condition, and creating symlinks on Windows is
/// privileged.
#[cfg(unix)]
#[test]
fn strict_symlink_spelled_tool_root_matches_the_real_discovered_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = real_temp_root(&dir);

    let store_ts = root.join("store").join("typescript");
    let real_lib = store_ts.join("lib");
    std::fs::create_dir_all(&real_lib).unwrap();
    let real_js = real_lib.join("tsserver.js");
    std::fs::write(&real_js, "// tsserver").unwrap();

    // `<pkg>/node_modules/typescript` -> `<store>/typescript`: the pnpm layout.
    let link_parent = root.join("pkg").join("node_modules");
    std::fs::create_dir_all(&link_parent).unwrap();
    let link = link_parent.join("typescript");
    std::os::unix::fs::symlink(&store_ts, &link).unwrap();

    let spelled_lib = link.join("lib");
    let spelled_js = spelled_lib
        .join("tsserver.js")
        .to_string_lossy()
        .to_string();
    let real_js_s = real_js.to_string_lossy().to_string();
    let disc = real_js_s.clone();

    let (used, plan) = resolve_tsserver_with(
        &NativeFs::new(),
        &tool_root_tsserver(
            Some(spelled_lib.to_string_lossy().as_ref()),
            Some(&spelled_js),
        ),
        "/ws",
        true,
        &|| Some("/usr/bin/node".to_string()),
        &|_, _| Some(disc.clone()),
    )
    .expect("a symlinked spelling of the pinned tsserver.js must resolve, not read as a mismatch");

    assert_eq!(
        used,
        canonicalize_path(&real_js_s),
        "the published tool root is the file's real path, not the symlinked spelling",
    );
    assert_ne!(
        used,
        canonicalize_path(&spelled_js),
        "the symlinked spelling must not survive into the internal form",
    );
    match plan {
        SpawnPlan::Tsserver { tsserver_js, .. } => assert_eq!(
            tsserver_js,
            canonicalize_path(&real_js_s),
            "the spawn plan runs the real tsserver.js",
        ),
        SpawnPlan::Tsgo { .. } => panic!("tsserver resolution must plan a tsserver spawn"),
    }
}

// ── non-strict skip-with-reason ──────────────────────────────────────────

#[test]
fn non_strict_missing_provider_skips_with_recorded_reason() {
    let no_node = || None;
    let no_tsgo = || None;
    let no_ts = |_: &str, _: &str| None;
    let res = resolve_with(
        &NativeFs::new(),
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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
        &NativeFs::new(),
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
