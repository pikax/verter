//! Fail-closed = SURFACE-the-failure discriminating tests for the in-memory
//! tsgo `--api` typecheck path.
//!
//! The `--noEmit` typecheck backend is tsgo-`--api`-only with NO tsc fallback.
//! "Fail closed" must mean the failure is SURFACED (non-zero process exit + a
//! clear stderr note), NOT swallowed: an ABSENT / broken / wire-diverged engine
//! that returned empty diagnostics and exit 0 would falsely report a clean
//! typecheck — a broken engine masquerading as "no type errors". This test pins
//! the opposite: when the `--api` engine cannot be discovered, verter-tsc exits
//! NON-ZERO.
//!
//! Hermetic: the child's `VERTER_TSGO_BIN` is removed, the temp project has no
//! `node_modules` engine, and the child's `PATH` is an empty directory so the
//! resolver's PATH tier cannot find a host engine; the update cache holds no
//! downloaded engine and no bundled sidecar ships next to the test binary, so the
//! engine is genuinely absent regardless of what is installed on the host.

use std::process::Command;

/// Build a minimal temp project: one `.vue` file listed in a tsconfig, and NO
/// `node_modules` engine. Returns `(temp_dir_guard, tsconfig_path)`.
fn engine_absent_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let root = temp.path().join("project");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");

    std::fs::write(
        src.join("App.vue"),
        "<script setup lang=\"ts\">\nconst props = defineProps<{ msg: string }>()\n</script>\n<template><div>{{ props.msg }}</div></template>\n",
    )
    .expect("write vue");

    let tsconfig_path = root.join("tsconfig.json");
    std::fs::write(
        &tsconfig_path,
        "{\n  \"compilerOptions\": { \"strict\": true },\n  \"files\": [\"src/App.vue\"]\n}\n",
    )
    .expect("write tsconfig");

    (temp, tsconfig_path)
}

/// DISCRIMINATING: with the `--api` engine genuinely absent, the `--noEmit`
/// typecheck cannot produce diagnostics. It MUST fail closed by SURFACING the
/// failure — a non-zero exit — never return empty diagnostics and exit 0 (which
/// falsely advertises a clean typecheck).
///
/// RED before the fix (the engine-absent path returned `Vec::new()` ⇒ no error
/// diagnostics ⇒ process exit 0 = success). GREEN after (exit non-zero).
#[test]
fn noemit_typecheck_exits_nonzero_when_api_engine_absent() {
    let (temp, tsconfig_path) = engine_absent_project();

    let bin = env!("CARGO_BIN_EXE_verter-tsc");
    // An empty PATH dir: the resolver's PATH tier cannot leak a host engine
    // into this hermetic absence.
    let empty_path = tempfile::TempDir::new().expect("empty PATH dir");
    let output = Command::new(bin)
        .arg("--noEmit")
        .arg("-p")
        .arg(&tsconfig_path)
        // Remove any host override so discovery genuinely fails (hermetic).
        .env_remove("VERTER_TSGO_BIN")
        .env("PATH", empty_path.path())
        .output()
        .expect("failed to execute verter-tsc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The core contract: a broken/missing typecheck engine must NOT masquerade as
    // a clean typecheck (exit 0). It must surface as a non-zero exit.
    assert!(
        !output.status.success(),
        "engine-absent `--noEmit` typecheck must exit NON-ZERO (fail-closed = surface the \
         failure), but exited {:?} (success). A missing engine masquerading as a clean \
         typecheck is exactly the quiet-failure this pins.\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}",
        output.status.code()
    );

    // And the failure is EXPLAINED on stderr (not a bare non-zero).
    assert!(
        stderr.contains("--api") || stderr.contains("engine"),
        "stderr must explain the engine-absent typecheck failure: {stderr}"
    );

    drop(temp);
}
