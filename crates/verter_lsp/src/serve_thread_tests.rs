//! The thread that polls `outbound::serve` must have a stack large enough for a
//! request handler, on every profile.
//!
//! The serve loop polls handler futures inline on the `block_on` thread, so
//! that thread's stack — not a runtime worker's — is what every LSP request
//! runs on. Under `#[tokio::main]` it is the process main thread, and on
//! Windows/MSVC that is the linker default 1 MiB, which a debug build's nested
//! `async fn` poll frames (measured ~1.8 MiB for one `textDocument/definition`)
//! exceed before the handler body even starts.

use super::{run_on_serve_thread, SERVE_THREAD_STACK_BYTES};

const CHILD_MARKER: &str = "VERTER_SERVE_THREAD_STACK_CHILD";

/// Stack the probe must be able to consume on the serve thread.
///
/// Above the 1 MiB Windows main-thread reserve — so a serve thread that is not
/// explicitly sized (or is sized too small) fails — and comfortably below
/// [`SERVE_THREAD_STACK_BYTES`], so a correctly sized thread never runs close to
/// its own limit.
const PROBE_BYTES: usize = 2 * 1024 * 1024;
const FRAME_BYTES: usize = 64 * 1024;

/// Touch `depth * FRAME_BYTES` of stack. Each frame keeps a real buffer alive
/// across the recursive call and both writes and reads it through `black_box`,
/// so the frame cannot be elided, merged, or turned into a tail call.
fn consume_stack(depth: usize) -> u8 {
    let mut frame = [0u8; FRAME_BYTES];
    frame[0] = depth as u8;
    frame[FRAME_BYTES - 1] = depth as u8;
    std::hint::black_box(&mut frame);
    let deeper = if depth == 0 {
        0
    } else {
        consume_stack(depth - 1)
    };
    std::hint::black_box(frame[FRAME_BYTES - 1]).wrapping_add(deeper)
}

/// The regression: the serve thread must survive a handler-sized stack demand.
///
/// Run in an isolated child process because a stack overflow aborts the whole
/// process — in the child it surfaces as a failing exit status instead of
/// taking the test binary down with it.
#[test]
fn serve_thread_stack_admits_a_handler_sized_frame() {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("1") {
        // No guard on PROBE_BYTES vs SERVE_THREAD_STACK_BYTES here: shrinking the
        // configured stack must drive this child into a real stack overflow, which
        // is the failure this test exists to catch. The size relationship itself is
        // pinned by `serve_thread_stack_clears_the_measured_debug_peak`.
        let touched = run_on_serve_thread(|| consume_stack(PROBE_BYTES / FRAME_BYTES));
        std::hint::black_box(touched);
        return;
    }

    let exe = std::env::current_exe().expect("current unit-test executable");
    let status = std::process::Command::new(exe)
        .arg("--exact")
        .arg("serve_thread_tests::serve_thread_stack_admits_a_handler_sized_frame")
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        // The serve thread's size must come from `SERVE_THREAD_STACK_BYTES`, never
        // from an ambient variable a user or CI may not set.
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated serve-thread stack child");

    assert!(
        status.success(),
        "the serve thread must admit a {PROBE_BYTES}-byte handler frame; a non-zero \
         status here is that thread exhausting its stack; status={status}"
    );
}

/// The configured size is what the doc comment justifies from measurement:
/// enough for the measured debug-profile peak of one request with real headroom.
#[test]
fn serve_thread_stack_clears_the_measured_debug_peak() {
    const MEASURED_DEBUG_PEAK_BYTES: usize = 1857 * 1024;
    const {
        assert!(SERVE_THREAD_STACK_BYTES >= 4 * MEASURED_DEBUG_PEAK_BYTES);
    }
}

/// Control: `run_on_serve_thread` returns its body's value and propagates a
/// panic, so replacing the entry point does not silently swallow either.
#[test]
fn serve_thread_returns_value_and_propagates_panic() {
    assert_eq!(run_on_serve_thread(|| 7u32 + 1), 8);

    let panicked = std::panic::catch_unwind(|| {
        run_on_serve_thread(|| panic!("serve body failed"));
    });
    assert!(
        panicked.is_err(),
        "a panic inside the serve body must reach the caller"
    );
}

const EXIT_CHILD_MARKER: &str = "VERTER_SERVE_EXIT_CHILD";

/// How long the background task stays inside `block_in_place` after the serve
/// body has returned — long enough that the runtime would be torn down while it
/// is still there.
const BLOCKING_WORK: std::time::Duration = std::time::Duration::from_millis(500);

/// The regression: after `exit`, a background task that is still inside
/// `tokio::task::block_in_place` must never be polled against a shut-down
/// runtime.
///
/// Dropping the runtime shuts its timer driver down without waiting for such a
/// task. When the blocking closure returns, the task keeps polling on its own
/// thread, and its next timer poll panics ("A Tokio 1.x context was found, but
/// it is being shutdown"). The child process plays that sequence: a spawned
/// task enters `block_in_place`, the serve body returns, and the task's next
/// await is a timer.
#[test]
fn serve_exit_never_polls_background_work_on_a_shut_down_runtime() {
    if std::env::var(EXIT_CHILD_MARKER).as_deref() == Ok("1") {
        crate::serve_until_exit(|| async {
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                tokio::task::block_in_place(|| {
                    let _ = entered_tx.send(());
                    std::thread::sleep(BLOCKING_WORK);
                });
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            });
            entered_rx
                .await
                .expect("the background task must enter block_in_place");
        });
    }

    let exe = std::env::current_exe().expect("current unit-test executable");
    let output = std::process::Command::new(exe)
        .arg("--exact")
        .arg("serve_thread_tests::serve_exit_never_polls_background_work_on_a_shut_down_runtime")
        .arg("--nocapture")
        .env(EXIT_CHILD_MARKER, "1")
        .output()
        .expect("spawn isolated serve-exit child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the server process must exit cleanly once serving ends; status={}; stderr:\n{stderr}",
        output.status
    );
    assert!(
        !stderr.contains("panicked"),
        "background work still inside block_in_place at exit must not panic; stderr:\n{stderr}"
    );
}
