//! Static structural guard on the crate's binary targets.
//!
//! `fake_tsgo_heartbeat` is a TEST-SUPPORT helper the relay-shim lifecycle tests spawn as a
//! stand-in `tsgo` (`tests/shim_live.rs`, via `CARGO_BIN_EXE_fake_tsgo_heartbeat`). It must stay a
//! test-support target — sourced from the test tree, never a production/packaged binary — while the
//! ONLY production binary is `verter-relay-shim`, and the crate is never published to a registry.
//! This guard pins that classification statically by manifest structure.
//!
//! It does NOT close the residual named by `TODO(follow-up D-15)` in `Cargo.toml`: a production
//! `cargo build --release -p verter_relay_shim` still PRODUCES the fake bin's build artifact,
//! because no stable-cargo mechanism excludes a test-support bin from `cargo build --release`
//! (default features) while keeping it built for a no-features `cargo nextest run --workspace`
//! (default features) — `required-features` off silently voids the orphan tests (they pass
//! vacuously when the fake bin is absent), and `-Z bindeps`/artifact-dependencies is unstable.

/// Parse the `name` + `path` of each `[[bin]]` block in the crate manifest. A block runs from its
/// `[[bin]]` header to the next section header (`\n[`), so intervening comment lines never match a
/// `name`/`path` field.
fn bin_targets(manifest: &str) -> Vec<(String, String)> {
    let mut bins = Vec::new();
    let mut rest = manifest;
    while let Some(idx) = rest.find("[[bin]]") {
        let after = &rest[idx + "[[bin]]".len()..];
        let block_end = after.find("\n[").unwrap_or(after.len());
        let block = &after[..block_end];
        let field = |key: &str| -> Option<String> {
            let prefix = format!("{key} =");
            block.lines().find_map(|line| {
                line.trim()
                    .strip_prefix(&prefix)
                    .map(|value| value.trim().trim_matches('"').to_string())
            })
        };
        if let (Some(name), Some(path)) = (field("name"), field("path")) {
            bins.push((name, path));
        }
        rest = &after[block_end..];
    }
    bins
}

/// The RAW text of the `[[bin]]` block whose `name` field equals `want` (from the `[[bin]]` header
/// to the next section header), or `None` if no such block exists. Reuses the same block bounds as
/// [`bin_targets`], so intervening comment lines never leak into another block.
fn bin_block<'a>(manifest: &'a str, want: &str) -> Option<&'a str> {
    let mut rest = manifest;
    while let Some(idx) = rest.find("[[bin]]") {
        let after = &rest[idx + "[[bin]]".len()..];
        let block_end = after.find("\n[").unwrap_or(after.len());
        let block = &after[..block_end];
        let name = block.lines().find_map(|line| {
            line.trim()
                .strip_prefix("name =")
                .map(|value| value.trim().trim_matches('"').to_string())
        });
        if name.as_deref() == Some(want) {
            return Some(block);
        }
        rest = &after[block_end..];
    }
    None
}

/// D-15 partial — the fake tsgo helper stays a test-support target (sourced from the test tree),
/// the sole production binary is `verter-relay-shim`, and the crate is unpublished. Discriminating:
/// this FAILS if `publish = false` is dropped, if a second `src/`-sourced production bin is added,
/// or if the fake helper is moved out of the test tree into `src/`.
#[test]
fn relay_shim_bins_are_one_production_shim_plus_a_test_support_fake() {
    let manifest = include_str!("../Cargo.toml");

    // (1) The crate is never published to a registry, so the test-support fake bin can never ship
    //     via crates.io regardless of the build-artifact residual documented above.
    assert!(
        manifest
            .lines()
            .any(|line| line.trim() == "publish = false"),
        "verter_relay_shim must stay `publish = false` — the test-support fake bin must never be \
         registry-publishable"
    );

    let bins = bin_targets(manifest);
    assert!(
        !bins.is_empty(),
        "expected explicit [[bin]] targets in the manifest; parsed none"
    );

    // (2) Exactly ONE binary is sourced from production `src/`, and it is the shim.
    let production: Vec<&(String, String)> = bins
        .iter()
        .filter(|(_, path)| path.starts_with("src/"))
        .collect();
    assert_eq!(
        production.len(),
        1,
        "exactly one production (src/-sourced) binary is expected; found {production:?}"
    );
    assert_eq!(
        production[0].0, "verter-relay-shim",
        "the sole production binary must be `verter-relay-shim`; found {:?}",
        production[0]
    );

    // (3) The fake tsgo helper is a TEST-SUPPORT target: sourced from the test tree (`tests/`),
    //     never from `src/`, so it is structurally a test helper, not a production binary.
    let fake = bins
        .iter()
        .find(|(name, _)| name == "fake_tsgo_heartbeat")
        .expect("the `fake_tsgo_heartbeat` test-support bin must be declared");
    assert!(
        fake.1.starts_with("tests/"),
        "the fake tsgo helper must be sourced from the test tree (tests/…), proving it is \
         test-support and not a production binary; found path {:?}",
        fake.1
    );
}

/// D-15 hardening — the exact regressions the D-15 residual documents must be REJECTED, not merely
/// documented. Two of them silently void the orphan tests:
///
/// 1. Adding `required-features` to `fake_tsgo_heartbeat` gates it behind a feature, so under the
///    no-features canonical gate the fake bin is ABSENT — the orphan tests' child-spawn fails and
///    their exit-non-zero / no-heartbeat-growth assertions pass VACUOUSLY. The block must carry no
///    `required-features` key at all.
/// 2. A `src/bin` binary would be an auto-discovered PRODUCTION binary the `[[bin]]`-block parse
///    cannot see (auto-discovered targets are not in the manifest). Cargo auto-discovers BOTH the
///    file form `src/bin/*.rs` AND the directory form `src/bin/<name>/main.rs`. The manifest pins
///    `autobins = false` to disable that discovery entirely, and — belt-and-suspenders — NEITHER
///    `src/bin` form may exist on disk, so the sole production bin stays the explicitly-declared
///    `verter-relay-shim` even if `autobins = false` were ever dropped.
///
/// Discriminating: FAILS if `fake_tsgo_heartbeat` gains `required-features`, if `autobins = false`
/// is dropped from the manifest, or if a `src/bin/*.rs` file OR a `src/bin/<name>/main.rs` directory
/// appears.
#[test]
fn fake_bin_has_no_required_features_and_no_auto_src_bins() {
    let manifest = include_str!("../Cargo.toml");
    let fake = bin_block(manifest, "fake_tsgo_heartbeat")
        .expect("the `fake_tsgo_heartbeat` [[bin]] block must be declared");
    assert!(
        !fake.contains("required-features"),
        "the `fake_tsgo_heartbeat` test-support bin must NOT carry `required-features`: gating it \
         behind a feature makes it ABSENT under the no-features canonical gate, silently voiding \
         the orphan tests (they pass vacuously when the fake bin is missing). Block: {fake:?}"
    );

    // The manifest must pin `autobins = false` so Cargo auto-discovers NO `src/bin` binary (neither
    // the `src/bin/*.rs` file form nor the `src/bin/<name>/main.rs` directory form) — the crate's
    // bins are exactly the explicit `[[bin]]` entries. Asserting it here stops the setting silently
    // regressing back to the default-on state.
    assert!(
        manifest
            .lines()
            .any(|line| line.trim() == "autobins = false"),
        "verter_relay_shim must pin `autobins = false` in [package]; otherwise Cargo auto-discovers \
         any `src/bin/*.rs` file or `src/bin/<name>/main.rs` directory as an unguarded production \
         binary the [[bin]]-block parse cannot see"
    );

    // Belt-and-suspenders: even if `autobins = false` were ever dropped, NO `src/bin` binary may
    // exist on disk. Cargo auto-discovers BOTH the file form `src/bin/<name>.rs` AND the directory
    // form `src/bin/<name>/main.rs`, so reject both.
    let src_bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("bin");
    let auto_bins: Vec<String> = match std::fs::read_dir(&src_bin) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let file_type = entry.file_type().ok()?;
                if file_type.is_file() && name.ends_with(".rs") {
                    // File form: `src/bin/<name>.rs`.
                    Some(name)
                } else if file_type.is_dir() && src_bin.join(&name).join("main.rs").is_file() {
                    // Directory form: `src/bin/<name>/main.rs`.
                    Some(format!("{name}/main.rs"))
                } else {
                    None
                }
            })
            .collect(),
        // No `src/bin` directory → no auto-discovered bins (the expected final state).
        Err(_) => Vec::new(),
    };
    assert!(
        auto_bins.is_empty(),
        "no auto-discovered `src/bin` production binary may exist (neither the `src/bin/*.rs` file \
         form nor the `src/bin/<name>/main.rs` directory form); found {auto_bins:?} at {src_bin:?} \
         — the sole production bin must stay `verter-relay-shim`"
    );
}
