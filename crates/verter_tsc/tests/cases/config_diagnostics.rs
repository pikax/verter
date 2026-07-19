//! Whole-program CONFIG / compiler-options diagnostics through the verter-tsc
//! `--noEmit` path.
//!
//! The in-memory `--api` typecheck collects `getConfigFileParsingDiagnostics`
//! (options/global diagnostics NOT covered by the per-file semantic/syntactic
//! getters) and surfaces them alongside the program diagnostics. This drives the
//! real `verter-tsc` binary over a project whose (extended) tsconfig sets an
//! INVALID `target` and asserts the resulting TS6046 is surfaced — the old path,
//! which queried only per-`root_files` semantic/syntactic diagnostics, never
//! collected config diagnostics at all.
//!
//! Gating mirrors the Rail B parity oracle: needs `packages/example/node_modules`
//! (for `vue` resolution) AND the gated rc `--api` engine (an explicit
//! `VERTER_TSGO_BIN` wins, else shared discovery against the workspace root). A
//! genuinely-absent engine SKIPs (the diagnostic is engine-specific), never a
//! vacuous pass.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Resolve the gated rc `--api` engine through the 4-tier toolchain resolver
/// (`VERTER_TSGO_BIN` wins; then shared PATH, project-local `node_modules`,
/// the update cache, and the bundled sidecar), capability-validated (bounded
/// version probe + support policy + a `--lsp` capability smoke per candidate).
fn resolve_rc_engine() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    verter_tsgo_api::toolchain::discovery::resolve_blocking(&request)
        .ok()
        .map(|resolution| resolution.path)
}

#[cfg(windows)]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(s) = status {
        if !s.success() {
            let _ = std::os::windows::fs::symlink_dir(src, dest);
        }
    }
}

#[cfg(not(windows))]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(src, dest);
}

/// Build a temp project with ONE clean `.vue`, a base tsconfig carrying a
/// deliberately INVALID `target`, and a `node_modules` junction for `vue`.
/// Returns `None` (skip) when the example node_modules is absent.
fn setup_bad_target_project() -> Option<(tempfile::TempDir, PathBuf)> {
    let node_modules_src = workspace_root()
        .join("packages")
        .join("example")
        .join("node_modules");
    if !node_modules_src.join("vue").exists() {
        eprintln!("SKIP: packages/example/node_modules/vue not found — run `pnpm install` first");
        return None;
    }

    let temp = tempfile::TempDir::new().expect("temp dir");
    let root = temp.path().to_path_buf();
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");

    // A clean SFC (its own type-check must produce no errors — the ONLY diagnostic
    // is the config-level bad-target one).
    std::fs::write(
        src.join("Clean.vue"),
        "<script setup lang=\"ts\">\nconst n: number = 1;\n</script>\n<template><div>{{ n }}</div></template>\n",
    )
    .expect("write Clean.vue");

    // Base tsconfig with an INVALID `target`. verter-tsc's synthetic config
    // `extends` this, so the bad option flows into config-parsing → TS6046.
    let tsconfig = root.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        "{\n  \"compilerOptions\": {\n    \"strict\": true,\n    \"target\": \"NotARealTarget\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"Bundler\",\n    \"skipLibCheck\": true\n  },\n  \"include\": [\"src\"]\n}\n",
    )
    .expect("write tsconfig");

    let nm_dest = root.join("node_modules");
    create_junction_or_symlink(&node_modules_src, &nm_dest);
    if !nm_dest.join("vue").exists() {
        eprintln!("SKIP: failed to create node_modules junction/symlink");
        return None;
    }

    Some((temp, tsconfig))
}

/// DISCRIMINATING: an invalid `target` surfaces TS6046 through the verter-tsc
/// `--noEmit` config-diagnostic collection.
///
/// RED before the whole-program change: `collect_diagnostics` queried only the
/// per-`root_files` semantic + syntactic getters and NEVER called
/// `getConfigFileParsingDiagnostics`, so the bad-target TS6046 was never
/// collected — it did not appear in the output. GREEN after: the config-parse
/// diagnostic is collected and surfaced.
#[test]
fn bad_target_option_surfaces_ts6046() {
    let Some((temp, tsconfig)) = setup_bad_target_project() else {
        return; // deps genuinely absent
    };
    let Some(rc_engine) = resolve_rc_engine() else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run \
             `pnpm install --frozen-lockfile`"
        );
        drop(temp);
        return;
    };

    let bin = env!("CARGO_BIN_EXE_verter-tsc");
    let output = Command::new(bin)
        .env("VERTER_TSGO_BIN", &rc_engine)
        .arg("--noEmit")
        .arg("-p")
        .arg(&tsconfig)
        .output()
        .expect("run verter-tsc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("=== STDOUT ===\n{stdout}\n=== STDERR ===\n{stderr}");

    // The bad `target` must surface as TS6046 (Argument for '--target' option must
    // be ...). The old per-root path never collected config diagnostics, so this
    // was absent.
    assert!(
        stdout.contains("TS6046"),
        "an invalid `target` must surface TS6046 via config-parse diagnostics \
         (the whole-program config collection); it was absent.\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );

    // And verter-tsc exits non-zero (diagnostics present).
    assert!(
        !output.status.success(),
        "a config error must make verter-tsc exit non-zero, got {:?}",
        output.status.code()
    );

    drop(temp);
}
