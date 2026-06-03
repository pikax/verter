//! Hover provenance opt-in + LRU cache.
//!
//! When `HoverOptions.provenance` is enabled, the
//! LSP attaches a provenance markdown section to hover responses,
//! showing which files the component-meta request loaded and the
//! derivation chain for the hovered binding.
//!
//! ## Architecture
//!
//! The enrichment itself is expensive (it runs an
//! `AuditedRequest` through the full semantic pipeline), so the
//! LSP returns the legacy payload immediately and spawns a
//! background task to compute the enriched payload. The next hover
//! at the same `(canonical_id, position)` returns the cached
//! enriched payload.
//!
//! The cache is an LRU bounded to 100 entries (`CACHE_CAPACITY`).
//! Access refreshes the LRU position. `invalidate_canonical(id)`
//! drops every entry whose key matches `id` — called on
//! `textDocument/didChange` for the changed file.
//!
//! ## Codified limitation
//!
//! Transitive dependencies are NOT tracked. If the hovered file's
//! *dependency* changes (e.g. `/c.vue` hovers a type imported from
//! `/types.ts` and `/types.ts` changes), the cache entry for
//! `/c.vue` is NOT invalidated. Consumers who need transitive
//! accuracy can edit the hovered file (triggers didChange →
//! invalidation) or disable the provenance feature.
//!
//! The `hover_provenance_cache_does_NOT_invalidate_on_transitive_dependency_change`
//! test pins this limitation so any future refactor that
//! accidentally expands the invalidation set will trip a
//! well-named failure.

use std::num::NonZeroUsize;

use lru::LruCache;
use parking_lot::Mutex;
use tower_lsp_server::ls_types::Position;
use verter_session::component_meta_audit::RequestAuditRecord;

/// Maximum number of hover-provenance entries retained in the cache.
/// LRU bounded at 100.
pub const CACHE_CAPACITY: usize = 100;

/// Cache key — `(canonical_id, position)` tuple. Position is UTF-16
/// LSP-style so the key matches what the hover handler observes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HoverProvenanceKey {
    pub canonical_id: String,
    pub line: u32,
    pub character: u32,
}

impl HoverProvenanceKey {
    pub fn new(canonical_id: impl Into<String>, position: Position) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            line: position.line,
            character: position.character,
        }
    }
}

/// An entry in the provenance cache: the markdown rendering of the
/// enriched section appended to the legacy hover body. Opaque string
/// — the LSP appends it verbatim below the legacy payload.
#[derive(Debug, Clone)]
pub struct HoverProvenancePayload {
    pub markdown: String,
}

/// Thread-safe LRU-100 cache of hover-provenance payloads, invalidated
/// on `didChange` for the affected canonical.
pub struct HoverProvenanceCache {
    inner: Mutex<LruCache<HoverProvenanceKey, HoverProvenancePayload>>,
}

impl HoverProvenanceCache {
    pub fn new() -> Self {
        Self::with_capacity(CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).expect("capacity.max(1) is always > 0");
        Self {
            inner: Mutex::new(LruCache::new(cap)),
        }
    }

    /// Insert a payload for the given key, evicting the LRU entry if
    /// at capacity.
    pub fn insert(&self, key: HoverProvenanceKey, payload: HoverProvenancePayload) {
        self.inner.lock().put(key, payload);
    }

    /// Look up a payload. On hit, refreshes the LRU position of the
    /// entry (subsequent calls keep it hot). On miss, returns `None`.
    #[must_use]
    pub fn get(&self, key: &HoverProvenanceKey) -> Option<HoverProvenancePayload> {
        self.inner.lock().get(key).cloned()
    }

    /// Remove every entry whose canonical matches `canonical_id`.
    /// Called on `textDocument/didChange` for the changed file.
    /// Transitive dependencies are NOT invalidated — that's the
    /// codified limitation.
    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let mut cache = self.inner.lock();
        // LruCache doesn't expose a drain-filter; rebuild a minimal
        // candidate list, then `pop` each — O(n) in cache size, which
        // is 100 worst case.
        let to_drop: Vec<HoverProvenanceKey> = cache
            .iter()
            .filter(|(k, _)| k.canonical_id == canonical_id)
            .map(|(k, _)| k.clone())
            .collect();
        for k in to_drop {
            cache.pop(&k);
        }
    }

    /// Test / diagnostics helper — current entry count.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Test / diagnostics helper — `true` when no entries are cached.
    /// Paired with `len()` to satisfy clippy `len_without_is_empty`
    /// and to document the canonical emptiness check (cache is empty
    /// if and only if `len() == 0`).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    /// Test helper — check if a key is present without refreshing LRU.
    #[cfg(test)]
    pub fn contains(&self, key: &HoverProvenanceKey) -> bool {
        self.inner.lock().peek(key).is_some()
    }
}

impl Default for HoverProvenanceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Render an audit record as a markdown "Provenance" section for
/// appending to a hover body. Pure formatting — does NOT walk the
/// graph (single-walker rule); it summarizes the footprint
/// counters and lists the union of `loaded_files()`.
#[must_use]
pub fn render_provenance_markdown(record: &RequestAuditRecord) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("\n\n---\n\n**Provenance**\n\n");
    if let Some(footprint) = record.footprint.as_ref() {
        let loaded = footprint.loaded_files();
        let _ = writeln!(out, "- Loaded files ({}):", loaded.len());
        for f in loaded.iter().take(10) {
            let _ = writeln!(out, "  - `{}`", f.as_ref());
        }
        if loaded.len() > 10 {
            let _ = writeln!(out, "  - _…and {} more_", loaded.len() - 10);
        }
        let tally = &footprint.cache_outcomes;
        let _ = writeln!(
            out,
            "- Cache: cold={} warm={} joined={} sentinels={}",
            tally.cold_builds, tally.warm_hits, tally.joined_waits, tally.sentinels,
        );
        let _ = writeln!(
            out,
            "- Instantiations: {}  •  Projections: {}  •  Conditionals: {}",
            footprint.instantiations.len(),
            footprint.projections.len(),
            footprint.conditional_decisions.len(),
        );
    } else {
        out.push_str(
            "- _Footprint not captured — enable `HostConfig::footprint_capture` \
             on the LSP host to see loaded files and derivation stats._\n",
        );
    }
    let _ = writeln!(
        out,
        "- `request_id = {}`  •  `canonical = {}`",
        record.request_id, record.canonical_id,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(s: &str) -> HoverProvenancePayload {
        HoverProvenancePayload {
            markdown: s.to_string(),
        }
    }

    fn pos(line: u32, ch: u32) -> Position {
        Position {
            line,
            character: ch,
        }
    }

    #[test]
    fn hover_provenance_disabled_by_default_returns_legacy_payload() {
        // The option surface defaults to `false` — the enrichment is
        // opt-in. Exercises the real
        // `HoverOptions::default()` AND the parse path on an empty
        // init payload. A regression that flips the default inverts
        // the opt-in contract — both assertions below fail in that
        // case. Discriminating against a constant flip OR a
        // silently-true `.unwrap_or(true)` fallback in the parser.
        use crate::config::{parse_hover_init_options, HoverOptions};

        assert!(
            !HoverOptions::default().provenance,
            "HoverOptions::default().provenance must be false — provenance is opt-in",
        );
        let parsed_empty = parse_hover_init_options(&serde_json::json!({}));
        assert!(
            !parsed_empty.provenance,
            "parse_hover_init_options on an empty payload must produce provenance=false",
        );
        let parsed_missing_hover_key =
            parse_hover_init_options(&serde_json::json!({ "other": true }));
        assert!(
            !parsed_missing_hover_key.provenance,
            "parse_hover_init_options with a foreign key must fall back to default=false",
        );
    }

    #[test]
    fn hover_provenance_enabled_returns_legacy_payload_then_cached_enriched_on_subsequent_hover() {
        // Pre-insert: miss. Post-insert: hit with the cached payload.
        // Models the "first hover returns legacy (cache miss), second
        // hover returns legacy + enriched (cache hit from background
        // task)" flow.
        let cache = HoverProvenanceCache::new();
        let key = HoverProvenanceKey::new("/c.vue", pos(0, 5));
        assert!(
            cache.get(&key).is_none(),
            "pre-insert: cache must miss (legacy-only hover)"
        );
        cache.insert(key.clone(), payload("## Why?\n- /c.vue"));
        let hit = cache.get(&key).expect("post-insert: cache must hit");
        assert_eq!(hit.markdown, "## Why?\n- /c.vue");
    }

    #[test]
    fn hover_provenance_cache_evicted_on_did_change_for_cached_canonical_file() {
        let cache = HoverProvenanceCache::new();
        let k_a = HoverProvenanceKey::new("/a.vue", pos(0, 0));
        let k_b = HoverProvenanceKey::new("/b.vue", pos(1, 2));
        cache.insert(k_a.clone(), payload("A"));
        cache.insert(k_b.clone(), payload("B"));
        assert_eq!(cache.len(), 2);

        cache.invalidate_canonical("/a.vue");
        assert!(
            !cache.contains(&k_a),
            "didChange on /a.vue must drop its cached entries"
        );
        assert!(
            cache.contains(&k_b),
            "didChange on /a.vue must NOT drop /b.vue entries"
        );
    }

    #[test]
    fn hover_provenance_cache_does_not_invalidate_on_transitive_dependency_change() {
        // Codified limitation. If this test starts
        // failing because a refactor expanded the invalidation set to
        // also drop transitive-dep entries, EITHER update the plan /
        // docs to reflect the broader contract, OR revert the
        // expansion. Do not silently delete this test.
        let cache = HoverProvenanceCache::new();
        let k_owner = HoverProvenanceKey::new("/c.vue", pos(3, 7));
        cache.insert(k_owner.clone(), payload("owner markdown"));
        // A dependency of /c.vue (say, /types.ts) changes.
        cache.invalidate_canonical("/types.ts");
        assert!(
            cache.contains(&k_owner),
            "transitive-dep change on /types.ts MUST NOT drop /c.vue's cached \
             provenance — this is the codified limitation. \
             If you intentionally widened invalidation, update the plan/docs \
             and rename this test rather than deleting it."
        );
    }

    #[test]
    fn hover_provenance_cache_lru_evicts_least_recently_used_at_100_entries() {
        // Fill exactly capacity, then insert one more — the
        // least-recently-used entry is evicted.
        let cache = HoverProvenanceCache::with_capacity(CACHE_CAPACITY);
        for i in 0..CACHE_CAPACITY {
            cache.insert(
                HoverProvenanceKey::new(format!("/f{i}.vue"), pos(0, 0)),
                payload(&format!("payload-{i}")),
            );
        }
        assert_eq!(cache.len(), CACHE_CAPACITY);
        let oldest = HoverProvenanceKey::new("/f0.vue", pos(0, 0));
        let newest = HoverProvenanceKey::new(format!("/f{}.vue", CACHE_CAPACITY - 1), pos(0, 0));
        assert!(cache.contains(&oldest), "oldest still present pre-overflow");
        assert!(cache.contains(&newest));

        // One more insert evicts the oldest.
        cache.insert(
            HoverProvenanceKey::new("/overflow.vue", pos(0, 0)),
            payload("overflow"),
        );
        assert_eq!(cache.len(), CACHE_CAPACITY);
        assert!(
            !cache.contains(&oldest),
            "oldest entry must have been evicted on overflow"
        );
        assert!(
            cache.contains(&HoverProvenanceKey::new("/overflow.vue", pos(0, 0))),
            "new entry must be present"
        );
    }

    #[test]
    fn render_provenance_markdown_handles_footprint_absent_and_present() {
        use verter_session::component_meta_audit::{
            ComponentMetaPayload, RequestAuditRecord, RequestFootprintAudit, RequestKind,
            RequestKindPayload, RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
        };

        let mut base = RequestAuditRecord {
            request_id: 42,
            canonical_id: "/c.vue".into(),
            kind: RequestKind::ComponentMeta,
            parent_request_id: None,
            timings: RequestTimingAudit::default(),
            store: RequestStoreAudit::default(),
            memory: RequestMemoryAudit::default(),
            footprint: None,
            scheduler: None,
            files: Vec::new(),
            waits: None,
            from_cache: false,
            kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
            trace_id: String::new(),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
        };

        // Missing footprint → the renderer surfaces a clear hint
        // about enabling footprint_capture rather than silently
        // producing empty output.
        let rendered = render_provenance_markdown(&base);
        assert!(
            rendered.contains("Provenance"),
            "header must be present: {rendered}"
        );
        assert!(
            rendered.contains("footprint_capture"),
            "must point at the capture flag when missing: {rendered}"
        );
        assert!(rendered.contains("request_id = 42"));

        // Present footprint → the renderer enumerates loaded files +
        // cache outcomes. This discriminates against
        // a stub-renderer that ignored `footprint.loaded_files()`.
        base.footprint = Some(RequestFootprintAudit::default());
        let rendered_empty = render_provenance_markdown(&base);
        assert!(rendered_empty.contains("Loaded files (0)"));
        assert!(
            !rendered_empty.contains("footprint_capture"),
            "must not emit the 'enable footprint_capture' hint when footprint is present: {rendered_empty}"
        );
    }

    #[test]
    fn hover_provenance_cache_access_refreshes_lru_position() {
        // Fill capacity. Access entry 0 (refreshes its LRU position).
        // Insert one more — the NEWLY least-recently-used entry
        // (which was entry 1, not entry 0) should be evicted; entry 0
        // survives despite being inserted first.
        let cache = HoverProvenanceCache::with_capacity(CACHE_CAPACITY);
        for i in 0..CACHE_CAPACITY {
            cache.insert(
                HoverProvenanceKey::new(format!("/f{i}.vue"), pos(0, 0)),
                payload(&format!("payload-{i}")),
            );
        }
        // Refresh entry 0 via `get()` — moves it to MRU position.
        let k0 = HoverProvenanceKey::new("/f0.vue", pos(0, 0));
        let _ = cache.get(&k0);

        // Now insert one more. The LRU entry should be /f1.vue
        // (NOT /f0.vue, which was just accessed).
        cache.insert(
            HoverProvenanceKey::new("/overflow.vue", pos(0, 0)),
            payload("overflow"),
        );
        assert!(
            cache.contains(&k0),
            "access-refreshed entry /f0.vue must survive overflow"
        );
        assert!(
            !cache.contains(&HoverProvenanceKey::new("/f1.vue", pos(0, 0))),
            "pre-existing oldest-after-refresh entry /f1.vue must have been evicted"
        );
    }
}
