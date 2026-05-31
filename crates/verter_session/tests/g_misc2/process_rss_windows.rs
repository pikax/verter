//! Windows-only RSS reporting test for `current_process_rss()`.
//!
//! This is a discriminating test per CLAUDE.md "Stub Prevention":
//! - Pre-change tree (Windows arm missing): function returns 0 → test FAILS.
//! - Post-change tree (Windows arm calls `K32GetProcessMemoryInfo`):
//!   function returns the live working-set size → test PASSES.
//!
//! The test allocates a 4 MB `Vec<u8>`, writes into every page so the
//! kernel actually backs it with physical memory (untouched virtual
//! pages do not count toward `WorkingSetSize`), then asserts the RSS
//! delta is at least 1 MB. 1 MB is the conservative floor for a
//! 4 MB allocation — Windows working-set accounting can settle to a
//! fraction of the requested size depending on scheduling, but a
//! genuinely-touched 4 MB buffer reliably moves WS by ≥ 1 MB on every
//! supported platform.
//!
//! Sibling negative: `process_rss_wasm.rs` covers the WASM 0 fallthrough.

#![cfg(target_os = "windows")]

use verter_session::component_meta_audit::current_process_rss;

#[test]
fn current_process_rss_reports_nonzero_working_set_on_windows() {
    // Force the working set into a known state by touching some memory
    // first (the test runner already has a non-trivial WS, but we want
    // the baseline to be observably-recent for the delta math).
    let baseline_pad = vec![0u8; 64 * 1024];
    std::hint::black_box(&baseline_pad);

    let pre = current_process_rss();
    assert!(
        pre > 0,
        "current_process_rss() must return a non-zero working-set size \
         on Windows after the K32GetProcessMemoryInfo arm lands; got {pre}",
    );

    // 4 MB allocation. We MUST write into every page so the kernel
    // actually backs the pages with physical memory; an uninitialized
    // `Vec::with_capacity` would not move WorkingSetSize.
    const ALLOC_BYTES: usize = 4 * 1024 * 1024;
    let mut buf: Vec<u8> = vec![0u8; ALLOC_BYTES];
    for chunk in buf.chunks_mut(4096) {
        // Write a non-zero, position-dependent byte so the optimizer
        // cannot prove the writes are dead.
        chunk[0] = (chunk.as_ptr() as usize & 0xFF) as u8;
    }
    std::hint::black_box(&buf);

    let post = current_process_rss();
    assert!(
        post > 0,
        "current_process_rss() must remain non-zero after allocation; got {post}",
    );

    let delta = post.saturating_sub(pre);
    const ONE_MB: u64 = 1024 * 1024;
    assert!(
        delta >= ONE_MB,
        "expected RSS delta of at least 1 MB after touching a 4 MB buffer; \
         got pre={pre} post={post} delta={delta}",
    );

    // Keep `buf` alive past both RSS reads.
    std::hint::black_box(&buf);
}
