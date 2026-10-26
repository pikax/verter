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

    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
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

// ── The engine is prepared in the background: its session runs while the
//    capability cross-check runs, and nothing is decided until the
//    cross-check passes. A rejected engine renders exactly the rejection the
//    in-order walk renders, even when the session already produced its
//    (empty) diagnostics, and no engine process outlives the run. ─────────────

/// Copy the fake engine into `dir` under its scenario name (the scenario is
/// selected by the binary's file name).
fn fake_engine_in(dir: &std::path::Path, scenario: &str) -> std::path::PathBuf {
    let name = if cfg!(windows) {
        format!("verter-tsgo-fake-{scenario}.exe")
    } else {
        format!("verter-tsgo-fake-{scenario}")
    };
    let target = dir.join(name);
    std::fs::copy(
        verter_test_support::cargo_test_binary_path!("verter_tsc_fake_engine"),
        &target,
    )
    .expect("copy the fake engine");
    target
}

/// The rejection line the in-order, fully validated walk renders for
/// `engine` given as `VERTER_TSGO_BIN`, with the per-process smoke directory
/// name masked.
fn in_order_rejection(engine: &std::path::Path) -> String {
    use verter_tsgo_api::toolchain::discovery::{
        resolve_blocking, ResolutionRequest, ResolveError,
    };
    use verter_tsgo_api::toolchain::validation::Capability;

    let mut request = ResolutionRequest::for_environment(Capability::Api, None);
    request.env_override = Some(engine.to_path_buf());
    request.path_entries = Vec::new();
    request.cache_root = None;
    request.host_exe = None;
    match resolve_blocking(&request) {
        Err(ResolveError::NoUsableCandidate { rejections, .. }) => {
            assert_eq!(rejections.len(), 1, "one candidate, one rejection");
            mask_smoke_dir(&format!("  - {}", rejections[0]))
        }
        other => panic!(
            "the in-order walk must reject {}: {other:?}",
            engine.display()
        ),
    }
}

/// Mask the capability smoke's per-process staging directory, the one part
/// of a rejection that differs between two runs.
fn mask_smoke_dir(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("verter-tsgo-smoke-") {
        out.push_str(&rest[..at]);
        out.push_str("verter-tsgo-smoke-<run>");
        rest = rest[at + "verter-tsgo-smoke-".len()..]
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    }
    out.push_str(rest);
    out
}

/// Run `verter-tsc --noEmit` on `tsconfig_path` with `engine` as
/// `VERTER_TSGO_BIN`, the fake's standalone `--api` session switched on, and
/// every engine process's pid recorded in `pid_dir`.
fn run_noemit_on(
    tsconfig_path: &std::path::Path,
    engine: &std::path::Path,
    pid_dir: &std::path::Path,
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let empty_path = tempfile::TempDir::new().expect("empty PATH dir");
    Command::new(verter_test_support::cargo_test_binary_path!("verter-tsc"))
        .envs(extra_env.iter().copied())
        .arg("--noEmit")
        .arg("-p")
        .arg(tsconfig_path)
        .env("VERTER_TSGO_BIN", engine)
        .env("VERTER_TSGO_FAKE_STANDALONE_API", "1")
        .env("VERTER_TSGO_FAKE_PID_DIR", pid_dir)
        .env("VERTER_TSGO_FAKE_STDERR", "1")
        .env("PATH", empty_path.path())
        .output()
        .expect("failed to execute verter-tsc")
}

/// How many times the fake engine announced a `mode` process on stderr.
fn announced(stderr: &str, mode: &str) -> usize {
    stderr
        .matches(&format!("verter-fake-engine: {mode} started"))
        .count()
}

/// The pids recorded in `pid_dir` for `mode` (`api` or `lsp`).
fn recorded_pids(pid_dir: &std::path::Path, mode: &str) -> Vec<u32> {
    std::fs::read_dir(pid_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_prefix(&format!("{mode}-"))?
                .strip_suffix(".pid")?
                .parse()
                .ok()
        })
        .collect()
}

/// No engine process recorded in `pid_dir` is still alive. Reaping is the
/// OS's business with no receipt to wait on, so this polls liveness under
/// one watchdog whose expiry is the failure.
fn assert_no_engine_survives(pid_dir: &std::path::Path) {
    use verter_tsgo_api::process::process_alive;
    let pids: Vec<u32> = recorded_pids(pid_dir, "api")
        .into_iter()
        .chain(recorded_pids(pid_dir, "lsp"))
        .collect();
    let watchdog = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while pids.iter().any(|pid| process_alive(*pid)) {
        assert!(
            std::time::Instant::now() < watchdog,
            "no engine process may outlive verter-tsc: {:?} still alive",
            pids.iter()
                .filter(|pid| process_alive(**pid))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Run a rejected `scenario` and check it fails closed with the in-order
/// walk's rejection. `passes_probe`: whether the scenario passes the version
/// probe, so that its session engine may start before the cross-check
/// rejects it and its capability check runs.
fn assert_rejected_like_the_in_order_walk(scenario: &str, passes_probe: bool) {
    let dir = tempfile::TempDir::new().expect("engine dir");
    let engine = fake_engine_in(dir.path(), scenario);
    let pid_dir = dir.path().join("pids");
    std::fs::create_dir_all(&pid_dir).expect("pid dir");
    let (project, tsconfig_path) = engine_absent_project();

    let output = run_noemit_on(&tsconfig_path, &engine, &pid_dir, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a rejected engine is an infrastructure failure (exit 2).\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
    assert!(
        stdout.trim().is_empty(),
        "nothing may be printed before the cross-check passes: {stdout}"
    );
    let expected = in_order_rejection(&engine);
    assert!(
        mask_smoke_dir(&stderr).contains(&expected),
        "the run must render exactly the in-order walk's rejection\n{expected}\nSTDERR:\n{stderr}"
    );
    if !passes_probe {
        assert!(
            recorded_pids(&pid_dir, "api").is_empty(),
            "no session engine starts for a candidate the version probe rejects"
        );
    }
    // The in-order walk never starts a session on a rejected engine, and
    // runs its capability check once: the speculative engines' stderr is
    // dropped, never shown.
    assert_eq!(
        announced(&stderr, "api"),
        0,
        "a rejected engine's session output must not be shown: {stderr}"
    );
    assert_eq!(
        announced(&stderr, "lsp"),
        usize::from(passes_probe),
        "the capability check's output is shown exactly once, as the in-order walk shows it: {stderr}"
    );
    assert_no_engine_survives(&pid_dir);
    drop(project);
}

// ── DISCRIMINATING: the attach succeeds but the in-band serverInfo.version
//    disagrees with the `--version` probe. The session engine answers with
//    (empty) diagnostics; without the cross-check gate the run exits 0. ─────
#[test]
fn a_server_info_version_mismatch_fails_closed() {
    assert_rejected_like_the_in_order_walk("apimismatch", true);
}

// ── DISCRIMINATING: the cross-check answers only after the session has
//    computed its (empty) diagnostics, and then rejects the engine: the run
//    still fails closed with the in-order walk's rejection. ──────────────────
#[test]
fn a_rejection_after_the_session_computed_diagnostics_still_fails_closed() {
    let dir = tempfile::TempDir::new().expect("engine dir");
    let engine = fake_engine_in(dir.path(), "apimismatch");
    let pid_dir = dir.path().join("pids");
    std::fs::create_dir_all(&pid_dir).expect("pid dir");
    let (project, tsconfig_path) = engine_absent_project();

    let output = run_noemit_on(
        &tsconfig_path,
        &engine,
        &pid_dir,
        &[("VERTER_TSGO_FAKE_LSP_DELAY_MS", "3000")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        std::fs::read_dir(&pid_dir)
            .expect("pid dir")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("diagnostics-")),
        "the session computed diagnostics before the cross-check answered"
    );
    assert_eq!(output.status.code(), Some(2), "STDERR:\n{stderr}");
    assert!(stdout.trim().is_empty(), "nothing may be printed: {stdout}");
    assert!(
        stderr.contains("does not match the `--version` probe"),
        "the run renders the cross-check's rejection: {stderr}"
    );
    assert_eq!(announced(&stderr, "api"), 0, "{stderr}");
    assert_no_engine_survives(&pid_dir);
    drop(project);
}

#[test]
fn a_dead_api_pipe_fails_closed() {
    assert_rejected_like_the_in_order_walk("ok", true);
}

#[test]
fn a_hollow_configured_project_fails_closed() {
    assert_rejected_like_the_in_order_walk("apihollow", true);
}

#[test]
fn an_engine_exiting_on_lsp_fails_closed() {
    assert_rejected_like_the_in_order_walk("exit", true);
}

#[test]
fn a_missing_server_info_fails_closed() {
    assert_rejected_like_the_in_order_walk("noserverinfo", true);
}

#[test]
fn an_out_of_policy_version_is_rejected_before_any_session() {
    assert_rejected_like_the_in_order_walk("v710", false);
}

// ── An engine whose cross-check passes is checked on the session engine the
//    preparation spawned, and no engine process outlives the run. ───────────
#[test]
fn an_accepted_engine_is_checked_and_leaves_no_engine_process() {
    let dir = tempfile::TempDir::new().expect("engine dir");
    let engine = fake_engine_in(dir.path(), "apiok");
    let pid_dir = dir.path().join("pids");
    std::fs::create_dir_all(&pid_dir).expect("pid dir");
    let (project, tsconfig_path) = engine_absent_project();

    let output = run_noemit_on(&tsconfig_path, &engine, &pid_dir, &[]);
    assert!(
        output.status.success(),
        "a working engine checks cleanly.\nSTDERR:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        recorded_pids(&pid_dir, "api").len(),
        1,
        "one session engine per run"
    );
    assert_eq!(
        recorded_pids(&pid_dir, "lsp").len(),
        1,
        "one cross-check engine per run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        (announced(&stderr, "api"), announced(&stderr, "lsp")),
        (1, 1),
        "an accepted engine's output is shown once it is accepted: {stderr}"
    );
    assert_no_engine_survives(&pid_dir);
    drop(project);
}

// ── DISCRIMINATING: a run that fails after the preparation started both
//    engines — the cross-check accepts the engine, but its session answers
//    with a hollow project set — exits with the session's failure and leaves
//    no engine process behind. Both engines have started by construction:
//    the session ran and the cross-check answered. ───────────────────────────
#[test]
fn a_run_whose_session_fails_after_both_engines_started_leaves_no_engine_process() {
    let dir = tempfile::TempDir::new().expect("engine dir");
    let engine = fake_engine_in(dir.path(), "apiok");
    let pid_dir = dir.path().join("pids");
    std::fs::create_dir_all(&pid_dir).expect("pid dir");
    let (project, tsconfig_path) = engine_absent_project();

    let output = run_noemit_on(
        &tsconfig_path,
        &engine,
        &pid_dir,
        &[("VERTER_TSGO_FAKE_STANDALONE_HOLLOW", "1")],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "STDERR:\n{stderr}");
    assert!(
        stderr.contains("did not open the configured project"),
        "the run fails on its session: {stderr}"
    );
    assert_eq!(
        (
            recorded_pids(&pid_dir, "api").len(),
            recorded_pids(&pid_dir, "lsp").len()
        ),
        (1, 1),
        "both engines started before the failure"
    );
    assert_no_engine_survives(&pid_dir);
    drop(project);
}
