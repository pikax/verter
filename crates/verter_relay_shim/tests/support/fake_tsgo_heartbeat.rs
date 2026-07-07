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

use std::io::Write;
use std::time::{Duration, Instant};

fn main() {
    let path = std::env::var("FAKE_TSGO_HEARTBEAT_FILE")
        .expect("fake_tsgo_heartbeat requires the FAKE_TSGO_HEARTBEAT_FILE env var");

    // Optional (Unix): after a brief warm-up, raise this signal on SELF, standing in for a
    // real tsgo that dies from a signal (an engine crash). A test uses it to prove the
    // shim faithfully re-raises a child's signal-exit rather than masking it as success.
    #[cfg(unix)]
    let raise_signal: Option<i32> = std::env::var("FAKE_TSGO_RAISE_SIGNAL")
        .ok()
        .and_then(|value| value.parse().ok());
    #[cfg(unix)]
    let started = Instant::now();

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
        if let Some(signum) = raise_signal {
            if started.elapsed() >= Duration::from_millis(150) {
                // SAFETY: raising a signal on the current process is async-signal-safe;
                // with no handler installed the default action terminates this process.
                unsafe {
                    libc::raise(signum);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
