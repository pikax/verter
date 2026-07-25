//! A portable, test-only fake `tsgo` child for the relay-shim lifecycle tests.
//!
//! It stands in for the real `tsgo` so the child-ownership tests run on EVERY
//! platform without a real engine. On start it appends one byte to the heartbeat
//! file named by `FAKE_TSGO_HEARTBEAT_FILE` every ~30ms and otherwise runs
//! forever, so it exits ONLY when its owner kills it (or after a hard safety cap,
//! so a bug can never leak an immortal process). That lets a test distinguish an
//! orphaned child (heartbeat keeps growing) from a reaped one (heartbeat stops).
//!
//! It deliberately IGNORES stdin/stdout and its forwarded args (`--lsp --stdio`):
//! the tests exercise the shim's PROCESS lifecycle, not the LSP wire.
//!
//! Two test-driven controls exist, both keyed to an OBSERVABLE event rather than a wall-clock delay
//! (see `RAISE_WHEN_FILE_ENV`): the Unix-only crash trigger, which raises a signal on self, and the
//! PORTABLE exit trigger (see `EXIT_WHEN_FILE_ENV`), which returns a normal exit code on every
//! platform. Plus the `--fixture-source-hash` freshness probe (see [`FIXTURE_SOURCE`]).
//!
//! EVERY path-valued input is read with [`path_var`], never `std::env::var` — see that function for
//! why the difference is load-bearing rather than stylistic.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// This fixture's OWN source text, baked in at the moment THIS BINARY was compiled.
///
/// It backs the `--fixture-source-hash` probe: the paired test
/// (`shim_live::fake_tsgo_fixture_binary_matches_its_source`) hashes the SAME checked-out file at
/// ITS own compile time and compares, so a binary built from different source text fails LOUDLY
/// instead of silently running underneath every live assertion in the suite. See that test for
/// what the probe does and does not protect against — measured, not assumed.
const FIXTURE_SOURCE: &str = include_str!("fake_tsgo_heartbeat.rs");

/// The signal the fixture raises on ITSELF to stand in for an engine crash. Unix-only.
/// Requires [`RAISE_WHEN_FILE_ENV`] — see [`resolve_raise_trigger`].
///
/// The raise site restores this signal's DEFAULT disposition and unblocks it first, so an inherited
/// `SIG_IGN` or an inherited blocked mask cannot turn the crash into a no-op — see the raise site in
/// [`main`] for why that inheritance is the fixture's problem to solve rather than its caller's.
#[cfg(unix)]
const RAISE_SIGNAL_ENV: &str = "FAKE_TSGO_RAISE_SIGNAL";

/// The path whose EXISTENCE triggers the crash.
///
/// **The crash anchor is this observable event, NEVER a wall-clock delay — do not "simplify" it
/// back to a sleep.** A delay measured from this process's own `main()` is unusable as a test
/// anchor: it silently includes fork+exec+dyld latency that the test cannot see or bound
/// (measured at ~112ms for this binary on a cold/loaded macOS machine, and it grows under
/// parallel test load).
///
/// The window that latency blew past is the shim's 200ms relay-stop grace check. That window does
/// NOT start at this process's spawn — it opens only when the shim's steady-state teardown select
/// resolves as `Teardown::RelayStopped` (the EDITOR DISCONNECT), and closes 200ms later. A crash
/// anchored to this process's own clock is therefore unordered against the event that opens the
/// window, and the test ends up measuring machine speed instead of shim behavior.
///
/// Anchoring on a file the TEST creates removes exec latency from the equation entirely and, more
/// importantly, makes the crash ORDERED with respect to the disconnect: the test disconnects, waits
/// for an OBSERVABLE witness that the shim has left its teardown select, and only then creates this
/// file — so the crash lands inside the window the disconnect opened.
#[cfg(unix)]
const RAISE_WHEN_FILE_ENV: &str = "FAKE_TSGO_RAISE_WHEN_FILE";

/// The heartbeat file this fixture appends to — the liveness witness every lifecycle test reads.
const HEARTBEAT_FILE_ENV: &str = "FAKE_TSGO_HEARTBEAT_FILE";

/// The exit CODE the fixture returns when [`EXIT_WHEN_FILE_ENV`] appears. Requires that variable —
/// see [`resolve_exit_trigger`].
const EXIT_CODE_ENV: &str = "FAKE_TSGO_EXIT_CODE";

/// The path whose EXISTENCE makes the fixture exit NORMALLY, with [`EXIT_CODE_ENV`]'s code.
///
/// The PORTABLE sibling of [`RAISE_WHEN_FILE_ENV`]: a signal-death is a Unix concept, but "the
/// engine terminated on its own" is not, and the shim's teardown must survive it on every platform.
/// Same anchoring discipline — an event the TEST causes, never a wall-clock delay.
const EXIT_WHEN_FILE_ENV: &str = "FAKE_TSGO_EXIT_WHEN_FILE";

// The freshness hash — ONE definition, shared with the paired test by `include!` so the two sides
// cannot drift apart.
include!("fnv1a64.rs");

/// The prefix the freshness probe prints. A CLOSED contract the paired test greps for.
const FIXTURE_SOURCE_HASH_PREFIX: &str = "FAKE_TSGO_FIXTURE_SOURCE_HASH:";

/// Read a PATH-valued variable as the OS gives it, never through a Unicode string.
///
/// `std::env::var` returns `Err(NotUnicode)` for a value that is not valid UTF-8, so the usual
/// `.ok()` / `.expect(..)` spellings turn an OS-legal path into either "unset" or a panic. Paths are
/// NOT guaranteed valid Unicode on any platform Verter supports — a Linux `TMPDIR` containing a
/// non-UTF-8 byte is enough — and the parent tests supply every one of these values from a
/// `PathBuf`. Decoding through `String` would therefore DISARM a trigger the test provably set (the
/// fail-closed half-configuration check below then kills the fixture) or abort it outright, and the
/// live test above it would fail through a symptom that names no cause.
///
/// `var_os` has no such failure mode: the bytes go in and come back out. Reserve `std::env::var` for
/// values that are genuinely textual or numeric, such as [`RAISE_SIGNAL_ENV`] and [`EXIT_CODE_ENV`].
fn path_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Resolve the crash trigger, FAILING CLOSED on a half-configured fixture.
///
/// A caller that asks for a crash signal but names no trigger file would otherwise get a fixture
/// that never crashes — a silent vacuous pass in exactly the tests whose whole point is that a
/// crash is propagated. Panic instead, so the misuse is loud.
#[cfg(unix)]
fn resolve_raise_trigger() -> Option<(i32, PathBuf)> {
    // The signal NUMBER is genuinely numeric text, so `var` is the right reader for it; the trigger
    // PATH is not, and goes through `path_var`.
    let signal = std::env::var(RAISE_SIGNAL_ENV).ok();
    let when_file = path_var(RAISE_WHEN_FILE_ENV);
    match (signal, when_file) {
        (None, None) => None,
        (Some(signal), Some(when_file)) => {
            let signum: i32 = signal.parse().unwrap_or_else(|e| {
                panic!("{RAISE_SIGNAL_ENV}={signal:?} is not a signal number: {e}")
            });
            Some((signum, when_file))
        }
        (Some(_), None) => panic!(
            "{RAISE_SIGNAL_ENV} is set without {RAISE_WHEN_FILE_ENV}: the fixture would never \
             crash and the test would pass vacuously"
        ),
        (None, Some(_)) => {
            panic!("{RAISE_WHEN_FILE_ENV} is set without {RAISE_SIGNAL_ENV}: no signal to raise")
        }
    }
}

/// Resolve the PORTABLE exit trigger, with the same fail-closed discipline as the crash trigger.
///
/// A configured exit CODE without a trigger file is a fixture that never exits — the vacuous pass
/// the shutdown tests exist to rule out — so it panics rather than running forever.
fn resolve_exit_trigger() -> Option<(i32, PathBuf)> {
    let code = std::env::var(EXIT_CODE_ENV).ok();
    let when_file = path_var(EXIT_WHEN_FILE_ENV);
    match (code, when_file) {
        (None, None) => None,
        (Some(code), Some(when_file)) => {
            let exit_code: i32 = code
                .parse()
                .unwrap_or_else(|e| panic!("{EXIT_CODE_ENV}={code:?} is not an exit code: {e}"));
            Some((exit_code, when_file))
        }
        (Some(_), None) => panic!(
            "{EXIT_CODE_ENV} is set without {EXIT_WHEN_FILE_ENV}: the fixture would never exit and \
             the test would pass vacuously"
        ),
        (None, Some(_)) => {
            panic!("{EXIT_WHEN_FILE_ENV} is set without {EXIT_CODE_ENV}: no exit code to return")
        }
    }
}

fn main() {
    // The staleness probe: print this binary's baked-in source hash and exit. Checked BEFORE the
    // heartbeat env var so the probe needs no fixture configuration at all.
    // `args_os`, not `args`: `std::env::args` PANICS on an argument that is not valid Unicode, and
    // this fixture is launched as the relay shim's engine with the editor's forwarded argv — the
    // same "paths are bytes" hazard as the environment values below, in the other OS-supplied
    // channel.
    if std::env::args_os()
        .skip(1)
        .any(|a| a == "--fixture-source-hash")
    {
        println!(
            "{FIXTURE_SOURCE_HASH_PREFIX}{:016x}",
            fnv1a64(FIXTURE_SOURCE.as_bytes())
        );
        return;
    }

    let path = path_var(HEARTBEAT_FILE_ENV)
        .unwrap_or_else(|| panic!("fake_tsgo_heartbeat requires the {HEARTBEAT_FILE_ENV} env var"));

    // Unix only: raise this signal on SELF the moment the trigger file appears, standing in for a
    // real tsgo that dies from a signal (an engine crash). The trigger is an event the TEST
    // causes and orders relative to the editor disconnect — never a wall-clock delay. See
    // `RAISE_WHEN_FILE_ENV` for why that distinction is load-bearing.
    #[cfg(unix)]
    let raise_trigger = resolve_raise_trigger();

    // Every platform: exit NORMALLY with a chosen code the moment the trigger file appears. Same
    // event-anchoring as the crash trigger, without the Unix-only signal.
    let exit_trigger = resolve_exit_trigger();

    // A hard safety cap: even if this child is orphaned (the very bug the tests
    // guard against, observed pre-fix), it self-terminates so no immortal process
    // leaks out of a test run. The tests complete in a couple of seconds.
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = file.write_all(b".");
            let _ = file.flush();
        }
        #[cfg(unix)]
        if let Some((signum, when_file)) = raise_trigger.as_ref() {
            if when_file.exists() {
                // Restore the DEFAULT disposition and UNBLOCK the signal before raising it, the same
                // way the shim's own signal-exit does. A disposition of `SIG_IGN` and a blocked
                // signal mask both SURVIVE `exec`, so this fixture inherits whatever its spawner had:
                // launched from a supervisor, a CI runner, or a shell that ran `trap '' TERM`, the
                // raise would return without terminating the process and the fixture would keep
                // beating. That turns "the engine crashed" into a silent no-op, and every test that
                // reads the death as its observable — including the base-negative control that treats
                // a raise as unreachable past a few beats — would then pass for the wrong reason.
                // SAFETY: libc signal primitives on a valid signal number, called from the fixture's
                // only thread with no handler machinery installed.
                unsafe {
                    let mut act: libc::sigaction = std::mem::zeroed();
                    act.sa_sigaction = libc::SIG_DFL;
                    libc::sigemptyset(&mut act.sa_mask);
                    act.sa_flags = 0;
                    libc::sigaction(*signum, &act, std::ptr::null_mut());

                    let mut unblock: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&mut unblock);
                    libc::sigaddset(&mut unblock, *signum);
                    libc::sigprocmask(libc::SIG_UNBLOCK, &unblock, std::ptr::null_mut());

                    libc::raise(*signum);
                }
            }
        }
        if let Some((code, when_file)) = exit_trigger.as_ref() {
            if when_file.exists() {
                std::process::exit(*code);
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
