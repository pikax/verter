//! The driver of the relocated-`bin_exe!` control, housed where it CANNOT reach the compile-time
//! build tree.
//!
//! The control it drives ([`super::shim_live`]'s
//! `the_bin_exe_macro_itself_honours_a_relocated_runtime_path` and
//! `the_relocation_control_does_not_read_the_compile_time_build_tree`) simulates a relocated
//! archive: real copies of both launched bins are placed in a temp directory and the test binary
//! re-runs itself as a child with `NEXTEST_BIN_EXE_*` pointed at those copies.
//!
//! Its copy SOURCE is the load-bearing detail. Sourcing the copies from `CARGO_BIN_EXE_*` would
//! make the control unrunnable on the very host it models — a machine an archive was moved to has
//! no builder `target/` tree, so the copy fails in setup and the macro assertion never runs. So the
//! source is resolved through [`resolve_bin_exe`], which prefers the RUNTIME path, and the
//! build-tree fallback arrives as an injected parameter that
//! `the_relocation_control_does_not_read_the_compile_time_build_tree` points at a directory proven
//! not to exist.
//!
//! WHY THIS IS ITS OWN MODULE: that negative control is only worth something if the injected
//! parameter is provably CONSUMED. An absent-path assertion cannot show it — a driver that ignored
//! the parameter and called `compile_time_bin_path(name)` directly (the historical defect) would
//! copy the real build-tree binaries, spawn a child that succeeds, and leave the absent directory
//! absent: green, with the control disarmed. `compile_time_bin_path` is therefore PRIVATE to
//! `shim_live`, and this module is a SIBLING rather than a child. Rust visibility is prefix-closed
//! downward, so a child (or any `pub(super)`) would inherit access; a sibling does not. Restoring
//! that direct call here is `E0603`/`E0425` under every path spelling — a compile error, not a
//! silent pass. The one spelling no visibility rule can seal is re-deriving the constant through
//! `env!`; that driver would ignore the injected provider, which is what the caller's
//! invocation-count assertion is for.

use super::shim_live::{
    resolve_bin_exe, ScopedTempDir, LAUNCHED_BINS, RELOCATED_BIN_DIR_VAR, RELOCATION_CHILD_TEST,
    RELOCATION_SENTINEL,
};

/// Run both halves of the relocation control.
///
/// `build_tree_fallback` yields the compile-time `CARGO_BIN_EXE_<name>` constant for a bin. It is
/// INJECTED rather than read here — and, per the module docs, CANNOT be read here — so that
/// `the_relocation_control_does_not_read_the_compile_time_build_tree` can drive this exact code path
/// with a build-tree path that provably does not exist: the layout of the relocated host this
/// control simulates. It is called exactly once per launched bin, which that test asserts.
///
/// Do NOT widen these imports to a glob (`use super::shim_live::*`) or re-home this module under
/// `shim_live`: either one puts `compile_time_bin_path` back in scope and turns the historical
/// defect from a compile error into a silent green.
pub(super) fn drive_relocation_control(build_tree_fallback: &dyn Fn(&str) -> String) {
    let relocated = ScopedTempDir::new("relocated_bins");
    let mut child = std::process::Command::new(
        std::env::current_exe().expect("the running test binary's own path"),
    );
    child
        .arg("--exact")
        .arg(RELOCATION_CHILD_TEST)
        .arg("--nocapture")
        // Assert the fail-closed arm's premise too: the child runs as if under nextest.
        .env("NEXTEST", "1")
        .env(RELOCATED_BIN_DIR_VAR, relocated.path());

    for name in LAUNCHED_BINS {
        // Source each copy from the path THIS process is actually running the bin from, resolved by
        // the same rule the macro uses: the runtime `NEXTEST_BIN_EXE_*` under nextest — the one
        // source present by definition inside a relocated archive — and the injected compile-time
        // constant only outside nextest, where nothing is relocated and `cargo test` has just built
        // it.
        //
        // Note this is NOT the macro: it calls `resolve_bin_exe` directly, so rewiring the macro
        // past the resolver leaves this setup intact and the child half free to catch it.
        let fallback = build_tree_fallback(name);
        let source = match resolve_bin_exe(
            name,
            std::env::var_os(format!("NEXTEST_BIN_EXE_{name}")),
            std::env::var_os("NEXTEST").is_some(),
            &fallback,
        ) {
            Ok(path) => path,
            Err(message) => panic!("{message}"),
        };
        let file_name = source
            .file_name()
            .expect("a bin path ends in a file name")
            .to_owned();
        // Copy rather than fabricate: the relocated copy must be a real, runnable binary so the
        // control models an extracted archive rather than a bare string swap. The platform's own
        // suffix (`.exe` on Windows) rides along in `file_name`, never hardcoded here.
        let destination = relocated.path().join(&file_name);
        std::fs::copy(&source, &destination).unwrap_or_else(|e| {
            panic!("copy {source:?} to {destination:?} to stand in for an extracted archive: {e}")
        });
        child.env(format!("NEXTEST_BIN_EXE_{name}"), &destination);
    }

    let output = child.output().expect("run the child half of the control");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    // `relocated` cleans itself up on every exit below, including the assertion panics.

    assert!(
        output.status.success(),
        "`bin_exe!` did not resolve to the relocated runtime paths: the macro must consult the \
         runtime NEXTEST_BIN_EXE_* value, never the compile-time build-tree constant.\n\
         --- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains(RELOCATION_SENTINEL),
        "the child half must have RUN its assertions — no {RELOCATION_SENTINEL:?} in its output, \
         so the control did not apply (a filter that matches nothing still exits 0).\n\
         --- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed"),
        "exactly the one filtered child test must have run; the `--exact {RELOCATION_CHILD_TEST}` \
         filter is out of date.\n--- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
    );
}
