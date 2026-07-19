//! B1: the declaration-emit stage resolves its engine through the SAME
//! first-working, capability-VALIDATED resolver as the typecheck stage (never
//! a `--version`-only selection that can mask a working candidate), and an
//! engine that FAILS the declaration invocation is a HARD failure (non-zero
//! exit) — never a silent `Ok` with zero diagnostics and zero declarations.
//!
//! Hermetic: the child's `PATH` is an empty directory (no host engine leaks
//! in), `VERTER_TSGO_BIN` / the temp project's `node_modules` carry copies of
//! the deterministic fake engine (`verter_tsc_fake_engine`, scenario selected
//! by file name). The fake's standalone `--api` surface lets the in-memory
//! typecheck stage pass with zero diagnostics so the run REACHES the
//! declaration stage.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VERTER_TSC: &str = env!("CARGO_BIN_EXE_verter-tsc");
const FAKE_ENGINE: &str = env!("CARGO_BIN_EXE_verter_tsc_fake_engine");

/// Copy the fake engine to a scenario-named path (the scenario is selected by
/// the binary's file name). Copies land via an atomic rename so parallel tests
/// never execute a partially-written file.
fn fake_engine(scenario: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("verter-tsc-fake-engines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fake engine dir");
    let name = if cfg!(windows) {
        format!("verter-tsgo-fake-{scenario}.exe")
    } else {
        format!("verter-tsgo-fake-{scenario}")
    };
    let target = dir.join(name);
    if !target.exists() {
        static COPY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = COPY_LOCK.lock().unwrap();
        if !target.exists() {
            let tmp = dir.join(format!(".copying-{scenario}"));
            std::fs::copy(FAKE_ENGINE, &tmp).expect("copy the fake engine");
            let _ = std::fs::remove_file(&target);
            std::fs::rename(&tmp, &target).expect("rename the fake engine into place");
        }
    }
    target
}

/// A minimal temp project: one `.vue` file listed in a tsconfig.
fn temp_project() -> (tempfile::TempDir, PathBuf) {
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

/// Plant `engine` as the host platform package inside the project's
/// `node_modules` (the project-local tier).
fn plant_project_local_engine(project_root: &Path, engine: &Path) -> PathBuf {
    let host = verter_tsgo_api::toolchain::platform::host_platform()
        .expect("test host is a supported platform");
    let dest = project_root
        .join("node_modules")
        .join(host.package_rel_path())
        .join(host.lib_executable_rel_path());
    std::fs::create_dir_all(dest.parent().unwrap()).expect("create package dirs");
    std::fs::copy(engine, &dest).expect("plant the project-local engine");
    dest
}

/// Run `verter-tsc --declaration` against the project with a hermetic engine
/// environment: `VERTER_TSGO_BIN` (when given) and an EMPTY `PATH`.
/// `fake_scenario` re-arms the fake's scenario via the child env for engines
/// planted under a fixed package name (`lib/tsc`).
fn run_declaration(
    tsconfig: &Path,
    declaration_dir: &Path,
    env_override: Option<&Path>,
    fake_scenario: Option<&str>,
) -> Output {
    let empty_path = tempfile::TempDir::new().expect("empty PATH dir");
    let mut cmd = Command::new(VERTER_TSC);
    cmd.arg("--declaration")
        .arg("--declarationDir")
        .arg(declaration_dir)
        .arg("-p")
        .arg(tsconfig)
        .env("PATH", empty_path.path());
    match env_override {
        Some(engine) => {
            cmd.env("VERTER_TSGO_BIN", engine);
        }
        None => {
            cmd.env_remove("VERTER_TSGO_BIN");
        }
    }
    if let Some(scenario) = fake_scenario {
        cmd.env("VERTER_TSGO_FAKE_SCENARIO", scenario);
    }
    cmd.output().expect("failed to execute verter-tsc")
}

// ── DISCRIMINATING (B1a): a tier-1 candidate that passes `--version` but
//    FAILS capability validation must NOT mask the working project-local
//    candidate. Today the declaration stage selects the first candidate passing
//    only `--version` + policy (the `mismatch` fake: version 7.0.2, serverInfo
//    disagrees), so its stderr names the WRONG engine; the validated resolver
//    skips it and names the working `apiok` engine. RED: stderr names
//    `verter-tsgo-fake-mismatch`. GREEN: stderr names `verter-tsgo-fake-apiok`.
#[test]
fn declaration_resolution_skips_a_version_passing_but_invalid_candidate() {
    let (_temp, tsconfig) = temp_project();
    let project_root = tsconfig.parent().unwrap().to_path_buf();
    let masked = fake_engine("mismatch");
    let working = plant_project_local_engine(&project_root, &fake_engine("apiok"));
    let out_dir = project_root.join("out");

    let output = run_declaration(&tsconfig, &out_dir, Some(&masked), Some("apiok"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the run must succeed with the working engine: {}\nSTDERR:\n{stderr}",
        output.status
    );
    let marker = stderr
        .lines()
        .find(|l| l.contains("declaration emit using tsgo at"))
        .unwrap_or_else(|| {
            panic!("the declaration stage must name its engine.\nSTDERR:\n{stderr}")
        });
    assert!(
        marker.contains("node_modules"),
        "the declaration stage must select the capability-VALIDATED project-local \
         engine ({}) — a `--version`-only selection is masked by the failing tier-1 \
         candidate.\nline: {marker}\nSTDERR:\n{stderr}",
        working.display()
    );
    assert!(
        !marker.contains("verter-tsgo-fake-mismatch"),
        "the version-passing-but-invalid candidate masked the working one: {marker}"
    );
}

// ── DISCRIMINATING (B1b): an engine that VALIDATES but then FAILS the
//    declaration invocation (the `declfail` fake exits 2 with no output) must
//    make verter-tsc exit NON-ZERO — never exit 0 with empty diagnostics and
//    zero declarations (a broken engine masquerading as a clean emit).
//    RED: today the invocation failure is swallowed into `Ok` → exit 0.
#[test]
fn declaration_invocation_failure_exits_nonzero_never_silent_success() {
    let (_temp, tsconfig) = temp_project();
    let project_root = tsconfig.parent().unwrap().to_path_buf();
    let engine = fake_engine("declfail");
    let out_dir = project_root.join("out");

    let output = run_declaration(&tsconfig, &out_dir, Some(&engine), None);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "an engine that fails the declaration invocation must exit NON-ZERO, but \
         verter-tsc exited {:?} (success) with no diagnostics — the silent-success \
         bug.\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}",
        output.status.code()
    );
    assert!(
        stderr.contains("declaration"),
        "stderr must explain the declaration-stage failure: {stderr}"
    );
}
