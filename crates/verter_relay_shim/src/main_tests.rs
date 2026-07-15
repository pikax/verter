use super::*;

fn tokens(args: &[&str]) -> std::vec::IntoIter<String> {
    args.iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

fn is_lsp(args: &ShimArgs) -> bool {
    args.forwarded.iter().any(|a| a == "--lsp")
}

/// A non-`--lsp` passthrough probe parses WITHOUT requiring the control rendezvous
/// args and routes to passthrough — not the relay, and NOT an arg error (the
/// advertised transparent-`tsc`-for-probes behaviour). Pre-fix this errored on the
/// missing `--control-dir`/`--session-key`.
#[test]
fn non_lsp_probe_parses_and_routes_to_passthrough() {
    let args = parse_args_from(tokens(&["--real-tsgo", "/opt/tsgo", "--", "--version"]))
        .expect("a `--real-tsgo X -- --version` probe must parse WITHOUT control args");
    assert_eq!(args.real_tsgo, PathBuf::from("/opt/tsgo"));
    assert!(
        args.control_dir.is_none(),
        "a probe requires no --control-dir"
    );
    assert!(
        args.session_key.is_none(),
        "a probe requires no --session-key"
    );
    assert_eq!(args.forwarded, vec!["--version".to_string()]);
    assert!(!is_lsp(&args), "no --lsp ⇒ the passthrough route is taken");
}

/// The `--lsp` relay path parses the control rendezvous args and is detected as the
/// relay route (the control args are enforced in `run_relay`).
#[test]
fn lsp_relay_parses_with_control_args() {
    let args = parse_args_from(tokens(&[
        "--real-tsgo",
        "/opt/tsgo",
        "--control-dir",
        "/tmp/ctl",
        "--session-key",
        "sess-1",
        "--",
        "--lsp",
        "--stdio",
    ]))
    .expect("the full --lsp relay invocation must parse");
    assert_eq!(args.control_dir, Some(PathBuf::from("/tmp/ctl")));
    assert_eq!(args.session_key.as_deref(), Some("sess-1"));
    assert_eq!(
        args.forwarded,
        vec!["--lsp".to_string(), "--stdio".to_string()]
    );
    assert!(is_lsp(&args), "--lsp ⇒ the relay route is taken");
}

#[test]
fn editor_owned_argv_uses_env_rendezvous_and_forwards_without_dashdash() {
    let mut args = tokens(&["--lsp", "--stdio"]);
    let parsed = parse_args_from_with_env(&mut args, |name| {
        Some(
            match name {
                REAL_TSGO_ENV => "/opt/real-tsgo",
                CONTROL_DIR_ENV => "/tmp/editor-control",
                SESSION_KEY_ENV => "editor-session",
                _ => return None,
            }
            .to_string(),
        )
    })
    .expect("an editor-selected shim must accept native tsgo argv with env rendezvous");

    assert_eq!(parsed.real_tsgo, PathBuf::from("/opt/real-tsgo"));
    assert_eq!(
        parsed.control_dir,
        Some(PathBuf::from("/tmp/editor-control"))
    );
    assert_eq!(parsed.session_key.as_deref(), Some("editor-session"));
    assert_eq!(parsed.forwarded, vec!["--lsp", "--stdio"]);
    assert!(is_lsp(&parsed));
}

#[test]
fn explicit_shim_args_override_editor_environment() {
    let mut args = tokens(&[
        "--real-tsgo",
        "/explicit/tsgo",
        "--control-dir",
        "/explicit/control",
        "--session-key",
        "explicit-session",
        "--",
        "--lsp",
    ]);
    let parsed = parse_args_from_with_env(&mut args, |_| Some("ignored-env".to_string())).unwrap();
    assert_eq!(parsed.real_tsgo, PathBuf::from("/explicit/tsgo"));
    assert_eq!(parsed.control_dir, Some(PathBuf::from("/explicit/control")));
    assert_eq!(parsed.session_key.as_deref(), Some("explicit-session"));
}

#[test]
fn editor_session_generation_is_exact_in_json_number_consumers() {
    // Advertisements are JSON and are inspected by editor-side JavaScript as
    // well as Rust. Keep the generation inside IEEE-754's exact integer range
    // so equality/identity checks cannot silently round the rendezvous witness.
    const MAX_JSON_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
    for _ in 0..16 {
        assert!(mint_generation() <= MAX_JSON_SAFE_INTEGER);
    }
}

/// The CLI contract shape is preserved: an unknown flag BEFORE `--` is still an
/// arg error (only the control-arg requirement moved to the `--lsp` path).
#[test]
fn unknown_flag_before_dashdash_is_still_an_error() {
    let err = parse_args_from(tokens(&["--real-tsgo", "/opt/tsgo", "--bogus"]))
        .expect_err("an unknown flag before -- must still error");
    assert!(
        err.contains("bogus"),
        "the error must name the unknown arg; got {err:?}"
    );
}

/// F4 — the `--verter-shim-identity` probe flag is recognized ONLY among the shim's OWN
/// top-level args; the scan STOPS at the first `--`, so a real-tsgo arg forwarded after `--` can
/// never trigger the identity probe. Pre-fix the scan matched ANYWHERE in argv (a bare
/// `.any(|a| a == "--verter-shim-identity")`), so a forwarded occurrence falsely triggered the
/// probe — the after-`--` case below FAILS against that logic.
#[test]
fn identity_probe_recognized_only_before_dashdash() {
    // The shim's own flag → probe.
    assert!(is_identity_probe(tokens(&["--verter-shim-identity"])));
    // Alongside other own args, before `--` → still a probe.
    assert!(is_identity_probe(tokens(&[
        "--real-tsgo",
        "/opt/tsgo",
        "--verter-shim-identity",
    ])));
    // AFTER `--` (a forwarded real-tsgo arg) → NOT a probe (the narrow contract).
    assert!(!is_identity_probe(tokens(&[
        "--real-tsgo",
        "/opt/tsgo",
        "--",
        "--verter-shim-identity",
    ])));
    // Only forwarded args after `--` carry it → not a probe.
    assert!(!is_identity_probe(tokens(&[
        "--",
        "--lsp",
        "--verter-shim-identity",
    ])));
    // Absent entirely → not a probe.
    assert!(!is_identity_probe(tokens(&[
        "--real-tsgo",
        "/opt/tsgo",
        "--",
        "--lsp",
    ])));
}

/// F2 — `contain_child_unix` must FAIL CLOSED when BOTH `setsid` AND `setpgid(0, 0)` fail: it
/// must `_exit(127)` rather than proceed, because a child that leads no process group of its own
/// makes a later `killpg(child_pid)` teardown MISS the child's subtree, leaving tsgo
/// grandchildren orphaned. The pre-fix body ignored the setpgid return
/// (`let _ = libc::setpgid(0, 0)`) and proceeded regardless, so this guard FAILS
/// against it. A live double-failure of setsid+setpgid is not portably inducible, so this is a
/// source-structure guard over the fn body (line comments stripped so prose never satisfies it).
#[test]
fn contain_child_unix_fails_closed_when_setsid_and_setpgid_both_fail() {
    let src = include_str!("main.rs");
    let start = src
        .find("fn contain_child_unix(")
        .expect("contain_child_unix is present");
    let end = src[start..]
        .find("struct ChildSetupGuard")
        .map(|off| start + off)
        .expect("ChildSetupGuard follows contain_child_unix");
    // Strip line comments so the fn's own doc/prose can never satisfy the code checks below.
    let body: String = src[start..end]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    // The setpgid return MUST be checked (as part of the double-failure condition), not discarded.
    assert!(
        body.contains("setpgid(0, 0) == -1"),
        "contain_child_unix must CHECK the setpgid(0, 0) return in the double-failure condition; \
             the ignored-return form leaves the child's subtree unreapable (killpg would miss it)"
    );
    assert!(
        !body.contains("let _ = libc::setpgid"),
        "contain_child_unix must NOT ignore the setpgid return (`let _ = libc::setpgid(...)`)"
    );
    // ...and the both-failed case must FAIL CLOSED via `_exit(127)` after that condition.
    let cond = body
        .find("libc::setsid() == -1 && libc::setpgid(0, 0) == -1")
        .expect("the setsid/setpgid double-failure must be a combined `&&` condition");
    let _exit = body[cond..]
        .find("libc::_exit(127)")
        .expect("the setsid/setpgid double-failure branch must `_exit(127)` (fail closed)");
}

/// F1 — the Unix shutdown-signal install MUST precede BOTH the child spawn AND the guard
/// disarm, and NOTHING fallible may sit between the disarm and the steady-state select. An
/// install AFTER the spawn leaves a spawn→install window: a signal in that window kills the
/// shim by the default disposition before the RAII guard can reap, orphaning the child. An
/// install after the `into_inner` disarm leaves the same orphan window at the tail. The runtime
/// orphan test cannot portably inject a signal into that microscopic window, so this is a
/// source-structure guard on the ordering invariant. Anchored within `run_relay` so the
/// passthrough `Command` (which uses `.status()`, not `.spawn()`) is never mistaken for the
/// relay child spawn.
#[test]
fn shutdown_signal_install_precedes_spawn_and_disarm() {
    let src = include_str!("main.rs");
    let run_relay = src
        .find("async fn run_relay(")
        .expect("run_relay is present");
    let region = &src[run_relay..];
    let install = region
        .find("ShutdownSignals::install()")
        .expect("the shutdown-signal install call site is present in run_relay");
    let spawn = region
        .find(".spawn()")
        .expect("the real-tsgo child spawn is present in run_relay");
    let disarm = region
        .find("child_guard.into_inner()")
        .expect("the guard disarm hand-off is present in run_relay");
    // The handlers must install BEFORE the child spawn — from the instant the child exists a
    // signal must be caught, never able to kill the shim by default and orphan the child.
    assert!(
        install < spawn,
        "the Unix shutdown-signal install (byte {install}) must run BEFORE the real-tsgo child \
             spawn (byte {spawn}) so no spawn→install window can orphan the child on a signal"
    );
    // ...and before the guard disarm, so a failed install or a setup-window signal unwinds
    // through the ARMED guard and never orphans the child.
    assert!(
        install < disarm,
        "the shutdown-signal install (byte {install}) must run BEFORE the guard disarm \
             (byte {disarm})"
    );
    // No fallible `?` may sit between the disarm and the steady-state select — the disarm
    // is the last fallible-free hand-off. Strip line comments so a prose `?` never trips it.
    let after_disarm = &region[disarm..];
    let select_off = after_disarm
        .find("let teardown =")
        .expect("the steady-state teardown select follows the disarm");
    let between_code: String = after_disarm[..select_off]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !between_code.contains('?'),
        "no fallible `?` may sit between the guard disarm and the steady-state select; \
             found one in: {between_code:?}"
    );
}

/// F1 — Windows containment must be ZERO-window: the shim self-assigns to the kill-on-close Job
/// Object BEFORE spawning tsgo, so on Win8+ tsgo is BORN into the job (a job member's children
/// join the job at creation, no breakaway limit set) rather than assigned in a post-spawn window
/// a `TerminateProcess` could race. Source-structure guard: within `run_relay`, the
/// `create_kill_on_close_job_and_self_assign()` call must precede the real-tsgo `.spawn()`. The
/// pre-fix code assigned the CHILD to the job AFTER spawn (no self-assign call existed at all), so
/// this guard FAILS against it. Region-bounded to `run_relay` so the fn DEFINITION above it and
/// this guard's own string literals never satisfy the check.
#[test]
fn windows_job_self_assign_precedes_spawn() {
    let src = include_str!("main.rs");
    let run_relay = src
        .find("async fn run_relay(")
        .expect("run_relay is present");
    let tests_mod = src.find("mod tests").expect("the tests module is present");
    // Strip line comments so a COMMENTED-OUT self-assign can never satisfy the check — only a
    // live call before the spawn counts.
    let region: String = src[run_relay..tests_mod]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let self_assign = region
        .find("create_kill_on_close_job_and_self_assign()")
        .expect(
            "run_relay must self-assign the shim to the kill-on-close job (zero-window Windows \
                 containment) via create_kill_on_close_job_and_self_assign()",
        );
    let spawn = region
        .find(".spawn()")
        .expect("the real-tsgo child spawn is present in run_relay");
    assert!(
        self_assign < spawn,
        "the Windows job self-assign (byte {self_assign}) must run BEFORE the real-tsgo spawn \
             (byte {spawn}) so tsgo is born into the kill-on-close job (zero spawn→assign window)"
    );
}

/// F7(a) — the Unix teardown select must be `biased` with the shutdown-signal arm FIRST,
/// so a signal already delivered to the shim is re-raised faithfully rather than losing to
/// an also-ready relay/child arm and exiting by code. An unbiased select (or a
/// signal-arm-last ordering) can drop a pending SIGTERM. Source-structure guard on the
/// tie-break ordering (a select! tie is not deterministically forceable at runtime).
#[test]
fn teardown_select_prioritizes_the_shutdown_signal() {
    let src = include_str!("main.rs");
    let signal_arm = src
        .find("signum = shutdown_signals.recv()")
        .expect("the unix teardown signal arm is present");
    let biased = src[..signal_arm]
        .rfind("biased;")
        .expect("the unix teardown select is `biased`");
    let child_arm = src[signal_arm..]
        .find("status = child.wait()")
        .map(|off| signal_arm + off)
        .expect("the child-exit arm follows the signal arm");
    assert!(
        biased < signal_arm,
        "the teardown select must be `biased` (byte {biased}) before the signal arm \
             (byte {signal_arm})"
    );
    assert!(
        signal_arm < child_arm,
        "the shutdown-signal arm (byte {signal_arm}) must be polled BEFORE the child/relay \
             arms (byte {child_arm}) — biased signal-priority"
    );
}

/// F3 — control-endpoint teardown must be DETERMINISTIC: after aborting the accept loop the
/// teardown must AWAIT the aborted task so the control listener (its UDS socket file on Unix /
/// its named pipe on Windows) is dropped BEFORE `run_relay` returns, rather than depending on
/// runtime drop timing. A runtime abort→drop race is not deterministically forceable, so this
/// is a source-structure guard: `accept_task.abort()` must be immediately followed by
/// `accept_task.await` (before the next teardown step). Pre-change the teardown aborted with NO
/// following await, so this guard FAILS against it. Line comments are stripped so a prose
/// `.await` mention can never satisfy the check.
#[test]
fn teardown_awaits_aborted_accept_task_before_return() {
    let src = include_str!("main.rs");
    let run_relay = src
        .find("async fn run_relay(")
        .expect("run_relay is present");
    // Bound the scan to production source (before the tests module) so this guard's own
    // string literals (`"accept_task.await"`, …) never satisfy the check.
    let tests_mod = src.find("mod tests").expect("the tests module is present");
    let region = &src[run_relay..tests_mod];
    let abort = region
        .find("accept_task.abort()")
        .expect("the teardown aborts the accept task");
    let after_abort: String = region[abort..]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let await_off = after_abort.find("accept_task.await").expect(
        "the aborted accept task must be AWAITED before teardown returns, so the control \
             listener (UDS socket / named pipe) is deterministically dropped rather than left to \
             runtime drop timing",
    );
    let remove_off = after_abort
        .find("remove_advertisement")
        .expect("the teardown removes the advertisement");
    assert!(
        await_off < remove_off,
        "accept_task.await (offset {await_off}) must immediately follow accept_task.abort() \
             and precede remove_advertisement (offset {remove_off}) so the listener is dropped \
             before return"
    );
}

/// F2 — the setup window must RACE a delivered shutdown signal against the fallible post-spawn
/// setup with SIGNAL PRIORITY, so a signal buffered during setup is re-raised faithfully rather
/// than masked as the `Code(1)` setup-error path. This is a `biased` select whose FIRST arm is
/// the shutdown-signal recv, placed in the SETUP region (before the guard disarm). Pre-change
/// setup ran linearly with no such select before the disarm, so this guard FAILS against it.
/// Source-structure guard (the microscopic real-timing race is not deterministically forceable
/// at runtime); the DECISION it feeds is unit-tested by `setup_signal_wins_over_later_setup_error`.
/// Region-bounded to production source so the guard's own string literals never satisfy it.
#[test]
fn setup_window_races_shutdown_signal_before_disarm() {
    let src = include_str!("main.rs");
    let run_relay = src
        .find("async fn run_relay(")
        .expect("run_relay is present");
    let tests_mod = src.find("mod tests").expect("the tests module is present");
    let region = &src[run_relay..tests_mod];
    let disarm = region
        .find("child_guard.into_inner()")
        .expect("the guard disarm is present");
    // The setup region is everything up to (excluding) the disarm.
    let setup_region = &region[..disarm];
    let signal_arm = setup_region
        .find("signum = shutdown_signals.recv()")
        .expect(
        "the setup window must RACE shutdown_signals.recv() BEFORE the guard disarm, so a signal \
             buffered during setup is re-raised rather than masked as Code(1)",
    );
    let biased = setup_region[..signal_arm]
        .rfind("biased;")
        .expect("the setup-window select must be `biased` (signal priority)");
    let setup_arm = setup_region[signal_arm..]
        .find("res = setup")
        .map(|off| signal_arm + off)
        .expect("the setup-completion arm follows the signal arm");
    assert!(
        biased < signal_arm,
        "the setup-window select must be `biased` (byte {biased}) before the signal arm (byte \
             {signal_arm})"
    );
    assert!(
        signal_arm < setup_arm,
        "the shutdown-signal arm (byte {signal_arm}) must be polled BEFORE the setup-completion \
             arm (byte {setup_arm}) — biased signal priority"
    );
}

/// F2 — the setup-window race DECISION: a shutdown signal delivered DURING setup is re-raised as
/// `Signal` (never masked as the `Code(1)` setup-error path), a setup error with no signal maps
/// to `Error` (→ `Code(1)` at `run`), and a clean setup maps to `Proceed`. UNIX-ONLY (the race,
/// the `Signalled`/`Signal` variants, and `ShimExit::Signal` are Unix-only). Discriminating: a
/// pre-fix "always error / always Code(1)" resolver FAILS the Signal and Proceed cases below.
/// Runs on the Linux/CI gate; compiles out on the Windows canonical gate (covered there by
/// cfg-correctness plus the `setup_window_races_shutdown_signal_before_disarm` source guard).
#[cfg(unix)]
#[test]
fn setup_signal_wins_over_later_setup_error() {
    // A delivered shutdown signal WINS — re-raised as Signal, NOT the Code(1) error path.
    match resolve_setup_race::<()>(SetupOutcome::Signalled(libc::SIGTERM), None) {
        SetupResolution::Signal(sig) => assert_eq!(sig, libc::SIGTERM),
        _ => panic!(
            "a delivered shutdown signal must resolve to Signal (re-raise), never the error path"
        ),
    }
    // A setup error with NO signal takes the error path (→ Code(1) at `run`).
    match resolve_setup_race::<()>(
        SetupOutcome::Done(Err("bind control endpoint: boom".into())),
        None,
    ) {
        SetupResolution::Error(message) => assert_eq!(message, "bind control endpoint: boom"),
        _ => panic!("a setup error with no signal must resolve to Error"),
    }
    // A clean setup proceeds to steady state with its handles.
    match resolve_setup_race::<u8>(SetupOutcome::Done(Ok(7)), None) {
        SetupResolution::Proceed(handles) => assert_eq!(handles, 7),
        _ => panic!("a clean setup must resolve to Proceed"),
    }
}

/// F3 — a shutdown signal that became PENDING during the SYNCHRONOUS setup body, when setup then
/// ERRORS, must re-raise as `Signal` rather than mask as the `Code(1)` setup-error path. The
/// setup-window select polls its signal arm only once, but the synchronous setup body has no
/// `.await` of its own, so a signal delivered mid-setup stays buffered and is re-polled
/// non-blockingly on the error path. Discriminating: the pre-fix resolver (mapping `Done(Err)` to
/// `Error` unconditionally, with no post-setup re-poll) FAILS the first case below. UNIX-ONLY.
#[cfg(unix)]
#[test]
fn setup_signal_pending_during_synchronous_setup_error_wins() {
    // A signal became pending during setup AND setup errored → re-raise the signal, not Code(1).
    match resolve_setup_race::<()>(
        SetupOutcome::Done(Err("bind control endpoint: boom".into())),
        Some(libc::SIGTERM),
    ) {
        SetupResolution::Signal(sig) => assert_eq!(sig, libc::SIGTERM),
        _ => panic!(
            "a signal pending during a synchronous setup that errored must re-raise as Signal, \
                 never the masked Code(1) error path"
        ),
    }
    // No signal pending → the setup error still takes the faithful error path.
    match resolve_setup_race::<()>(
        SetupOutcome::Done(Err("bind control endpoint: boom".into())),
        None,
    ) {
        SetupResolution::Error(message) => assert_eq!(message, "bind control endpoint: boom"),
        _ => panic!("a setup error with no pending signal must resolve to Error"),
    }
    // A CLEAN setup proceeds even with a concurrently-pending signal — steady state's biased
    // select observes it — so the pending signal is not consumed on the Ok path here.
    match resolve_setup_race::<u8>(SetupOutcome::Done(Ok(9)), Some(libc::SIGTERM)) {
        SetupResolution::Proceed(handles) => assert_eq!(handles, 9),
        _ => panic!("a clean setup proceeds; a pending signal is handled by steady state"),
    }
}

/// The setup-error signal recovery must use the DETERMINISTIC bounded await (`recv_pending_now`,
/// which turns the reactor on an awaited `recv()`), never a non-reactor-turning poll (`try_recv`
/// with a no-op waker). A signal delivered during the synchronous setup body is captured by tokio's
/// OS signal handler — written to the signal self-pipe — at DELIVERY; only a poll that never turns
/// the reactor made observing it a gamble. The awaited `recv()` is a real waker-driven wakeup, so a
/// buffered signal is drained deterministically; the bound only limits the genuinely-no-signal case.
/// The live signal-during-sync-setup race is not portably inducible, so the determinism is
/// by-construction and this source-structure guard pins the mechanism. Pre-fix the recovery called
/// `shutdown_signals.try_recv()` (the no-op-waker poll), so this guard FAILS against it.
/// Region-bounded to `run_relay` production source (before the tests module) with line comments
/// stripped, so this guard's own literals and any prose can never satisfy the check.
#[test]
fn setup_error_signal_recovery_uses_the_bounded_await() {
    let src = include_str!("main.rs");
    let run_relay = src
        .find("async fn run_relay(")
        .expect("run_relay is present");
    let tests_mod = src.find("mod tests").expect("the tests module is present");
    let region: String = src[run_relay..tests_mod]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    // The setup-error recovery must call the DETERMINISTIC bounded await, which turns the reactor.
    assert!(
        region.contains("SetupOutcome::Done(Err(_)) => shutdown_signals.recv_pending_now()"),
        "the setup-error path must recover a mid-setup signal via the DETERMINISTIC bounded await \
         recv_pending_now(...).await (which turns the reactor on the awaited recv()), not a \
         non-reactor-turning poll"
    );
    // ...and must NOT use the non-reactor-turning `try_recv` no-op-waker poll anywhere in run_relay.
    assert!(
        !region.contains("try_recv"),
        "the setup-error signal recovery must not use the non-reactor-turning try_recv() poll (a \
         no-op-waker poll that never turns the signal driver); use the bounded recv_pending_now"
    );
}

/// `ShutdownSignals::recv_pending_now` must stay the DETERMINISTIC bounded, reactor-turning drain:
/// a `tokio::time::timeout` around the awaited `self.recv()`. Turning the reactor on the awaited
/// `recv()` is what drains a signal delivered during synchronous setup — already written to the OS
/// signal self-pipe at delivery — on the first reactor turn; the bound only limits the
/// genuinely-no-signal case. A non-reactor poll (`try_recv`, a `Waker::noop` / `noop_waker` poll, or
/// a bare `poll_recv`) would turn observing a buffered signal back into a gamble. This
/// source-structure guard isolates the `recv_pending_now` body (line comments stripped, so its own
/// prose can never satisfy the check) and pins the mechanism: it FAILS if the body is rewritten to a
/// non-reactor poll under the same name.
#[test]
fn recv_pending_now_uses_the_bounded_reactor_turning_await() {
    let src = include_str!("main.rs");
    let start = src
        .find("async fn recv_pending_now")
        .expect("recv_pending_now is present");
    // Bound to the method body: its closing brace is the first line at the method's 4-space indent.
    let rel_end = src[start..]
        .find("\n    }")
        .expect("recv_pending_now body closes at the method's 4-space indent");
    let body: String = src[start..start + rel_end]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    // The drain MUST be the bounded, reactor-turning await: a timeout around the awaited recv().
    assert!(
        body.contains("tokio::time::timeout"),
        "recv_pending_now must bound the drain with tokio::time::timeout(...), so the \
         genuinely-no-signal case cannot block the setup-error path"
    );
    assert!(
        body.contains("self.recv()"),
        "recv_pending_now must await self.recv() — turning the reactor is what drains a signal \
         buffered on the OS self-pipe during synchronous setup"
    );
    // ...and MUST NOT degrade to a non-reactor poll, which would turn observing a buffered signal
    // into a gamble instead of a deterministic reactor-turning drain.
    for banned in ["try_recv", "Waker::noop", "noop_waker", "poll_recv"] {
        assert!(
            !body.contains(banned),
            "recv_pending_now must not use the non-reactor poll `{banned}`; keep the bounded \
             reactor-turning tokio::time::timeout(_, self.recv()).await drain"
        );
    }
}

/// G5 — the relay-stop kill classifier returns a clean `Code(0)` ONLY for OUR SIGKILL; a child
/// that died on its OWN (a self-signal or a non-zero self-exit in the race just after the grace
/// deadline) is propagated faithfully, never masked as a clean disconnect. UNIX-ONLY (constructs
/// raw wait statuses via `ExitStatusExt::from_raw`); the microscopic post-grace race is not
/// portably inducible in a live test, so the decision function is exercised directly.
///
/// Discriminating-by-construction: a classifier that blindly returns `Code(0)` after the
/// relay-stop kill (the pre-fix branch) FAILS the SIGTERM + non-zero-exit cases below.
#[cfg(unix)]
#[test]
fn relay_stop_kill_classifier_propagates_child_self_death_not_code_zero() {
    use std::os::unix::process::ExitStatusExt;

    // OUR SIGKILL of a still-alive child → a clean editor-disconnect exit.
    let killed = ExitStatus::from_raw(libc::SIGKILL);
    assert!(
        matches!(shim_exit_after_relay_stop_kill(killed), ShimExit::Code(0)),
        "our SIGKILL of a still-alive child is a clean Code(0) disconnect"
    );

    // The child self-signalled (an engine crash via SIGTERM) → re-raise the signal, not Code(0).
    let self_signalled = ExitStatus::from_raw(libc::SIGTERM);
    assert!(
        matches!(
            shim_exit_after_relay_stop_kill(self_signalled),
            ShimExit::Signal(sig) if sig == libc::SIGTERM
        ),
        "a child that died from its OWN SIGTERM must be re-raised, not masked as Code(0)"
    );

    // The child self-exited non-zero (a crash exit code) → propagate the code, not Code(0).
    // A raw wait status encodes the exit code in bits 8..16 (`code << 8`).
    let self_exited = ExitStatus::from_raw(42 << 8);
    assert!(
        matches!(
            shim_exit_after_relay_stop_kill(self_exited),
            ShimExit::Code(42)
        ),
        "a child that self-exited non-zero must propagate that code, not Code(0)"
    );
}

/// The terminal `exit` fn's Unix `Signal` arm must re-raise the signal via the HARDENED,
/// async-signal-safe sequence: install the DEFAULT disposition through `sigaction` (NOT the
/// weaker, less-portable `libc::signal`), UNBLOCK the signal (`sigprocmask` + `SIG_UNBLOCK`) so
/// an inherited mask can never suppress it, `raise` it, and — only if `raise` somehow returns —
/// fall through to a trailing `_exit(128 + signo)` guard rather than a normal return. Actually
/// re-raising terminates the test process, so this is a SOURCE-STRUCTURE guard (like
/// `teardown_awaits_aborted_accept_task_before_return`): it reads the source text and validates
/// the Unix arm's ordering on EVERY platform. The weaker prior body — `libc::signal(sig,
/// SIG_DFL); libc::raise(sig)` with no unblock and no `_exit` guard, under a fn named
/// `into_exit_code` returning `ExitCode` — has no `fn exit(self) -> !`, so this guard FAILS
/// against it. Line comments are stripped so a prose `//` mention can never satisfy the check.
#[test]
fn unix_signal_exit_uses_sigaction_unblock_raise_exit_guard() {
    let src = include_str!("main.rs");
    let tests_mod = src.find("mod tests").expect("the tests module is present");
    // Bound the scan to production source so this guard's own string literals never satisfy it.
    let prod = &src[..tests_mod];
    let exit_fn = prod
        .find("fn exit(self) -> !")
        .expect("the terminal `fn exit(self) -> !` is present");
    let region = &prod[exit_fn..];
    let signal_arm = region
        .find("ShimExit::Signal(sig) =>")
        .expect("the Unix Signal arm of `exit` is present");
    // Strip line comments so a prose `//` mention can never satisfy the ordering check.
    let arm_code: String = region[signal_arm..]
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let sigaction = arm_code
        .find("sigaction(")
        .expect("the Signal arm installs the default disposition via sigaction(...)");
    let sigprocmask = arm_code
        .find("sigprocmask(")
        .expect("the Signal arm unblocks the signal via sigprocmask(...)");
    let unblock = arm_code
        .find("SIG_UNBLOCK")
        .expect("the Signal arm unblocks with SIG_UNBLOCK");
    let raise = arm_code
        .find("raise(")
        .expect("the Signal arm re-raises the signal via raise(...)");
    let exit_guard = arm_code
        .find("_exit(")
        .expect("the Signal arm has a trailing _exit(...) guard");
    assert!(
        sigaction < sigprocmask,
        "sigaction (byte {sigaction}) must precede sigprocmask (byte {sigprocmask})"
    );
    assert!(
        unblock < raise && sigprocmask < raise,
        "the SIG_UNBLOCK sigprocmask (bytes {unblock}/{sigprocmask}) must precede the re-raise \
             (byte {raise})"
    );
    assert!(
        raise < exit_guard,
        "the re-raise (byte {raise}) must precede the trailing _exit guard (byte {exit_guard})"
    );
    assert!(
        !arm_code.contains("libc::signal("),
        "the Signal arm must install the default disposition via sigaction, NOT the weaker \
             libc::signal()"
    );
}

/// A Windows exit code WIDER than 8 bits must survive `from_status` VERBATIM — no `& 0xff`
/// truncation. Unix wait-status exit codes are 8-bit by POSIX, so the full-width concern is
/// genuinely a Windows one; this asserts the Windows path with a real NTSTATUS-shaped code
/// (`0xC0000005` = STATUS_ACCESS_VIOLATION). The `ShimExit::Code` field widened (`u8` → `i32`),
/// so the value is observed through the `Debug` channel — the test compiles against BOTH field
/// types and discriminates at RUNTIME: the prior `(code & 0xff) as u8` yields `Code(5)`, the
/// full-width mapping yields `Code(-1073741819)`.
#[cfg(windows)]
#[test]
fn windows_exit_status_preserves_full_code() {
    use std::os::windows::process::ExitStatusExt;
    let raw: u32 = 0xC000_0005;
    let full = raw as i32; // -1073741819
    let exit = ShimExit::from_status(ExitStatus::from_raw(raw));
    assert_eq!(
        format!("{exit:?}"),
        format!("Code({full})"),
        "the full-width Windows exit code must survive from_status (no & 0xff truncation to \
             Code(5))"
    );
}

/// Doc-accuracy guard for [`shim_exit_after_relay_stop_kill`]: the classifier attributes a bare
/// final `SIGKILL` after our relay-stop kill to OUR kill, but that attribution is genuinely
/// AMBIGUOUS — a child self-`SIGKILL` or an OOM-kill racing the teardown deadline is
/// indistinguishable from our kill via a wait status alone. The doc must state that residual
/// ambiguity PLAINLY and must NOT overclaim that the classifier authoritatively catches every
/// engine crash. A prior doc claimed the faithful path meant "an engine crash is never masked",
/// with no acknowledgment of the `SIGKILL` gap, so this guard FAILS against it. Source-structure
/// guard over the classifier's doc+body region (lower-cased for a case-insensitive match).
#[test]
fn relay_stop_sigkill_status_is_documented_ambiguous() {
    let src = include_str!("main.rs");
    let doc_start = src
        .find("/// Classify the child's FINAL status after a relay-stop kill")
        .expect("the relay-stop classifier doc is present");
    let region_end = src[doc_start..]
        .find("async fn passthrough")
        .map(|off| doc_start + off)
        .expect("passthrough follows the relay-stop classifier");
    let region = src[doc_start..region_end].to_ascii_lowercase();
    // The residual-ambiguity acknowledgment must be present.
    assert!(
        region.contains("ambiguit")
            || region.contains("cannot distinguish")
            || region.contains("residual"),
        "the relay-stop classifier doc must acknowledge the SIGKILL attribution ambiguity (a bare \
             SIGKILL is indistinguishable from a child self-SIGKILL / OOM-kill racing teardown)"
    );
    // ...and it must NOT overclaim authoritative crash detection.
    for overclaim in ["authoritative", "impossible to mask", "never masks"] {
        assert!(
            !region.contains(overclaim),
            "the relay-stop classifier doc must not overclaim ({overclaim:?}); the SIGKILL \
                 attribution gap is a known residual, not a guarantee"
        );
    }
}

/// The embedded identity marker carries the EXACT pinned prefix the packaging scanner greps for,
/// followed by a non-empty crate version. The packaging side depends on this literal prefix, so
/// it is locked here: a drifted prefix (or an empty version) fails this test. The retention
/// static must mirror the marker bytes so the literal is guaranteed present in the shipped
/// binary even under aggressive dead-code elimination.
#[test]
fn shim_identity_marker_has_pinned_prefix() {
    const PINNED_PREFIX: &str = "VERTER_RELAY_SHIM_IDENTITY:v1:";
    assert!(
        SHIM_IDENTITY.starts_with(PINNED_PREFIX),
        "the identity marker must carry the pinned prefix {PINNED_PREFIX:?}; got {SHIM_IDENTITY:?}"
    );
    assert!(
        SHIM_IDENTITY.len() > PINNED_PREFIX.len(),
        "the identity marker must include a non-empty version after the pinned prefix; got \
             {SHIM_IDENTITY:?}"
    );
    assert_eq!(
        SHIM_IDENTITY_MARKER,
        SHIM_IDENTITY.as_bytes(),
        "the retention static must mirror the identity marker bytes"
    );
}
