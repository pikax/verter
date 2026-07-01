//! Tests for the per-collection diagnostic-source cache: single resolution +
//! single line-index build per file (the D9 scaling contract), overlay-first
//! resolution order, and cached misses.

use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// A source backed by an external `Rc<Cell<usize>>` read counter, so the count
/// stays observable after the source is moved into the cache.
fn counting_source(
    reads: &Rc<Cell<usize>>,
    content: Option<Arc<str>>,
) -> impl Fn(&str) -> Option<Arc<str>> {
    let reads = Rc::clone(reads);
    move |_raw_path: &str| {
        reads.set(reads.get() + 1);
        content.clone()
    }
}

/// D9 (index build): N offset conversions in ONE file build the line index exactly
/// ONCE and reuse it — `line_index()` returns the SAME object every time. RED if
/// the index were rebuilt per access (distinct addresses).
#[test]
fn one_index_build_for_many_offsets_in_one_file() {
    let reads = Rc::new(Cell::new(0usize));
    let cache = DiagnosticSourceCache::new(counting_source(
        &reads,
        Some(Arc::from("line one\nline two\nline three\n")),
    ));

    let mut index_addrs = Vec::new();
    for _ in 0..5 {
        let sf = cache
            .source_file("c:/proj/src/big.ts")
            .expect("content resolves");
        let idx = sf.line_index();
        index_addrs.push(idx as *const Utf16LineIndex as usize);
        // A real offset query (exercises the built index).
        let _ = idx.line_col_for_utf16(10);
    }

    assert!(
        index_addrs.windows(2).all(|w| w[0] == w[1]),
        "the line index must be built ONCE and reused (same object across accesses): {index_addrs:?}"
    );
    // And the underlying content was resolved exactly once for the 5 accesses.
    assert_eq!(
        reads.get(),
        1,
        "N accesses to one file resolve the content exactly once (D9): got {}",
        reads.get()
    );
}

/// D9 (resolution count): the source is resolved exactly once across many
/// `source_file` calls for the same file identity (including separator /
/// drive-case variants). RED = one resolution per call.
#[test]
fn source_is_resolved_at_most_once_per_file_identity() {
    let reads = Rc::new(Cell::new(0usize));
    let cache = DiagnosticSourceCache::new(counting_source(&reads, Some(Arc::from("abc\ndef"))));

    for path in [
        "c:/proj/a.ts",
        "c:/proj/a.ts",
        r"c:\proj\a.ts",
        "C:/proj/a.ts",
    ] {
        assert!(cache.source_file(path).is_some());
    }
    assert_eq!(
        reads.get(),
        1,
        "one file identity across all its separator / drive-case forms resolves once (got {})",
        reads.get()
    );
}

/// A genuine miss is memoized: the source is consulted once, subsequent lookups
/// return `None` from the cache without re-resolving.
#[test]
fn miss_is_cached_and_not_re_resolved() {
    let reads = Rc::new(Cell::new(0usize));
    let cache = DiagnosticSourceCache::new(counting_source(&reads, None));

    assert!(cache.source_file("c:/proj/missing.ts").is_none());
    assert!(cache.source_file("c:/proj/missing.ts").is_none());
    assert!(cache.source_file(r"c:\proj\missing.ts").is_none());
    assert_eq!(
        reads.get(),
        1,
        "a miss must be resolved once and then served from the cache (never re-resolved)"
    );
}

/// Overlay-first: `OverlayThenFallback` serves an overlay entry (by the shared
/// identity key) and only falls through to the fallback on a miss.
#[test]
fn overlay_then_fallback_prefers_overlay_then_falls_through() {
    let fallback_reads = Rc::new(Cell::new(0usize));
    let fallback = counting_source(&fallback_reads, Some(Arc::from("from disk")));
    let source = OverlayThenFallback::new([("c:/proj/Carrier.vue.tsx", "from overlay")], fallback);
    let cache = DiagnosticSourceCache::new(source);

    // The overlay entry is served (even via a drive-case-divergent form) WITHOUT
    // touching the fallback.
    let sf = cache
        .source_file("C:/proj/Carrier.vue.tsx")
        .expect("overlay entry resolves");
    assert_eq!(sf.text(), "from overlay");
    assert_eq!(
        fallback_reads.get(),
        0,
        "an overlay hit must NOT consult the fallback"
    );

    // A path NOT in the overlay falls through to the fallback.
    let other = cache
        .source_file("c:/proj/other.ts")
        .expect("fallback resolves");
    assert_eq!(other.text(), "from disk");
    assert_eq!(fallback_reads.get(), 1, "a miss consults the fallback once");
}
