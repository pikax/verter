//! A per-collection diagnostic-source cache: resolve each file's content ONCE per
//! diagnostic pass and reuse a build-once [`Utf16LineIndex`] for every offset in
//! that file.
//!
//! A whole-program diagnostic set homes each diagnostic by reading the target
//! file's content and converting the UTF-16 `pos`/`end`. Done naively that is a
//! content read + a full offset walk PER diagnostic — O(diagnostics × length) when
//! many diagnostics share a file. This cache collapses that to ONE content
//! resolution + ONE line-index build per file, keyed by the shared
//! filesystem-identity key ([`InjectedPathKey`]) so a separator / drive-case
//! variant of the same path hits the same entry.
//!
//! The cache is generic over a [`DiagnosticContentSource`] so `verter_span` (the
//! leaf coordinate crate) does not need to know HOW content is resolved: each
//! consumer wires its own source (e.g. an overlay-first-then-disk source for the
//! `--noEmit` typecheck, or the `--lsp` content cache for the owned provider). It
//! is a per-request/per-collection structure (single-threaded within one pass);
//! it is NOT a long-lived shared cache.
//!
//! A genuine miss (the source cannot resolve the file) is a CACHED `None`, so the
//! consumer can surface it as an EXPLICIT mapping error rather than silently
//! fabricating a `(1,1)` / `(0,0)` position — a resolved miss is never re-attempted
//! and never guessed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::path::InjectedPathKey;
use crate::utf16_line_index::Utf16LineIndex;

/// Resolves the exact text the engine saw for a reported file path. Consumers
/// implement the resolution order they need (e.g. overlay carriers first, then the
/// real filesystem).
pub trait DiagnosticContentSource {
    /// Return the content for `raw_path`, or `None` if it cannot be resolved.
    ///
    /// `raw_path` is the file path exactly as the engine reported it (the caller
    /// has NOT normalized it — the source may normalize as it sees fit; the cache
    /// keys the RESULT by the shared identity key regardless).
    fn resolve(&self, raw_path: &str) -> Option<Arc<str>>;
}

/// A blanket impl so a plain closure can be used as a content source.
impl<F> DiagnosticContentSource for F
where
    F: Fn(&str) -> Option<Arc<str>>,
{
    fn resolve(&self, raw_path: &str) -> Option<Arc<str>> {
        self(raw_path)
    }
}

/// One resolved file: its content plus a lazily-built [`Utf16LineIndex`]. The index
/// is built at most once (on first offset demand) and reused for every subsequent
/// query on the same file.
#[derive(Debug)]
pub struct SourceFile {
    text: Arc<str>,
    index: OnceLock<Utf16LineIndex>,
}

impl SourceFile {
    fn new(text: Arc<str>) -> Self {
        Self {
            text,
            index: OnceLock::new(),
        }
    }

    /// The file's content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The build-once UTF-16 line index, constructed on first demand and reused
    /// thereafter (subsequent calls return the SAME index — never a rebuild).
    pub fn line_index(&self) -> &Utf16LineIndex {
        self.index
            .get_or_init(|| Utf16LineIndex::new(Arc::clone(&self.text)))
    }
}

/// A per-collection content + line-index cache, keyed by the shared
/// filesystem-identity key.
///
/// Both a HIT (`Some`) and a MISS (`None`) are memoized, so a file is resolved at
/// most once per pass and a miss is surfaced (not silently retried or guessed).
#[derive(Debug)]
pub struct DiagnosticSourceCache<S> {
    source: S,
    entries: RefCell<HashMap<InjectedPathKey, Option<Arc<SourceFile>>>>,
}

impl<S: DiagnosticContentSource> DiagnosticSourceCache<S> {
    /// Build a cache over `source`.
    pub fn new(source: S) -> Self {
        Self {
            source,
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Resolve `raw_path` to a [`SourceFile`], or `None` if the source cannot
    /// resolve it. The first call for a given file identity resolves through the
    /// source; every later call for the SAME identity (including a separator /
    /// drive-case variant) returns the cached result WITHOUT re-resolving.
    pub fn source_file(&self, raw_path: &str) -> Option<Arc<SourceFile>> {
        let key = InjectedPathKey::new(raw_path);
        if let Some(cached) = self.entries.borrow().get(&key) {
            return cached.clone();
        }
        // Resolve once and memoize (Some OR None). A borrowed guard is not held
        // across `resolve` so a source that itself touches the cache cannot
        // deadlock the RefCell.
        let resolved = self
            .source
            .resolve(raw_path)
            .map(SourceFile::new)
            .map(Arc::new);
        self.entries.borrow_mut().insert(key, resolved.clone());
        resolved
    }
}

/// A shared content source that layers a fixed in-memory OVERLAY over a fallback
/// source: it checks the overlay entries FIRST (by the shared filesystem-identity
/// key) and only falls through to the fallback on a miss. This is the exact
/// "overlay carriers, then real filesystem" order the `--noEmit` typecheck needs,
/// expressed once so both the resolution order and the identity folding are shared.
pub struct OverlayThenFallback<F> {
    overlay: HashMap<InjectedPathKey, Arc<str>>,
    fallback: F,
}

impl<F> OverlayThenFallback<F> {
    /// Build from `(raw_path, content)` overlay pairs and a fallback source.
    pub fn new<I, P, C>(overlay: I, fallback: F) -> Self
    where
        I: IntoIterator<Item = (P, C)>,
        P: AsRef<str>,
        C: Into<Arc<str>>,
    {
        Self {
            overlay: overlay
                .into_iter()
                .map(|(p, c)| (InjectedPathKey::new(p.as_ref()), c.into()))
                .collect(),
            fallback,
        }
    }
}

impl<F: DiagnosticContentSource> DiagnosticContentSource for OverlayThenFallback<F> {
    fn resolve(&self, raw_path: &str) -> Option<Arc<str>> {
        if let Some(content) = self.overlay.get(&InjectedPathKey::new(raw_path)) {
            return Some(Arc::clone(content));
        }
        self.fallback.resolve(raw_path)
    }
}

#[cfg(test)]
#[path = "diag_source_tests.rs"]
mod tests;
