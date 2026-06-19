//! Arch guard: `parking_lot::Semaphore` is NOT used in the scheduler src.
//!
//! `CpuConcurrencyPermit` is a hand-rolled
//! `parking_lot::Mutex<usize>` + `Condvar` permit counter. The project
//! forbids `parking_lot::Semaphore` — a hand-rolled counter gives the
//! RAII `#[must_use]` non-`Clone` permit semantics + panic-safe release
//! that the scheduler's per-task CPU-concurrency limiter requires, and
//! avoids a second accounting truth.
//!
//! This guard fires if anyone re-introduces a `parking_lot::Semaphore`
//! reference anywhere in `crates/verter_scheduler/src/`. It is a static
//! source scan over the whole tree (the same convention as
//! `dag_arch_guards.rs`), which proves *non-use across every module* —
//! stronger than a single trybuild fixture, which only proves one file
//! fails to compile.

use std::fs;
use std::path::{Path, PathBuf};

fn scheduler_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Reads every `.rs` file under `src/`, stripping line/doc comments so
/// the scan discriminates a *code* use of `parking_lot::Semaphore` from
/// prose that legitimately NAMES the forbidden type to explain why it is
/// banned (the `cpu_concurrency.rs` rationale comment does exactly that).
fn read_scheduler_source_code_only() -> String {
    let mut buf = String::new();
    walk(&scheduler_src_root(), &mut buf);
    buf
}

fn walk(dir: &Path, buf: &mut String) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk(&path, buf);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let raw = fs::read_to_string(&path).expect("read file");
            for line in raw.lines() {
                buf.push_str(strip_comment(line));
                buf.push('\n');
            }
        }
    }
}

/// Drops the `//`-introduced comment tail (covering `//`, `///`, `//!`).
fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// The scheduler src tree must not reference `parking_lot::Semaphore`.
///
/// Discriminator: the needle `parking_lot::Semaphore` and the bare
/// `use parking_lot::...Semaphore` form. The doc-comment mention of the
/// type inside *this* test file does not count (we scan `src/`, not
/// `tests/`).
#[test]
fn parking_lot_semaphore_not_used_in_scheduler_src() {
    let code = read_scheduler_source_code_only();
    // Qualified-path needles only. A bare `Semaphore` token is NOT used
    // because the crate's own `CpuConcurrencySemaphore` type legitimately
    // contains that substring. Any real use of the forbidden type surfaces
    // as a `parking_lot::Semaphore` path or a `use parking_lot::{...}`
    // import that names `Semaphore` after the segment separator — both
    // forms contain `lot::Semaphore`. `Mutex` / `Condvar` are the allowed
    // primitives and are unaffected.
    for needle in ["parking_lot::Semaphore", "lot::Semaphore"] {
        assert!(
            !code.contains(needle),
            "`{needle}` re-appeared in scheduler code — \
             the CPU-concurrency limiter MUST be the hand-rolled \
             `parking_lot::Mutex<usize>` + `Condvar` counter, never \
             `parking_lot::Semaphore`",
        );
    }
    // Also catch a grouped import `use parking_lot::{Mutex, Semaphore}`,
    // where `Semaphore` is preceded by a separator rather than `lot::`.
    for line in code.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("use parking_lot::") {
            assert!(
                !line.contains("Semaphore"),
                "a `use parking_lot::{{...}}` import names `Semaphore` — \
                 the CPU-concurrency limiter MUST be the hand-rolled \
                 `Mutex<usize>` + `Condvar` counter, never `parking_lot::Semaphore`",
            );
        }
    }
}
