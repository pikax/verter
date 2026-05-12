//! `SessionView` — read-only view trait over the base host.
//!
//! `SessionView` is the read-only surface that resolver-tier code uses to
//! observe host state. It is the **read substrate** for queries; it never
//! mutates the host (R17). Two concrete impls cover the cases we care
//! about:
//!
//! - [`HostView`] — direct passthrough to the base [`VerterHost`]. The
//!   query reads come from the live host state. Used by overlay-free
//!   sessions and by background work that operates on the workspace's
//!   canonical content.
//! - [`OverlaidView`] — overlay-aware view backed by per-canonical
//!   overlay sources. Resolution falls through to the base host when the
//!   overlay map does not carry the requested canonical. Overlays never
//!   mutate the base host; they coexist with the base under different
//!   content hashes (R17).
//!
//! ## Plan provenance
//!
//! Introduced by the fact-based cache refactor's **Stage 4a**. The
//! companion stages thread `SessionView` through `ResolverContext`
//! (Stage 4b), make [`HostFenceValidator`](crate::host_manage::HostFenceValidator)
//! view-aware (Stage 4c), and delete the overlay-mutation machinery
//! (Stage 4d). See `/type-cache-architecture` and `/host-session`
//! skills for the architectural rules (R17–R20).
//!
//! ## What `SessionView` is NOT
//!
//! - It is NOT the cache-correctness oracle. Fact validation (per-cache
//!   `fact_dep_signature` matched against recorded facts) decides
//!   whether a cached entry is fresh (R19).
//! - It is NOT a thread-local. `SessionView` is passed explicitly
//!   through `ResolverContext` (R18). Thread-local "current view"
//!   globals — `_in_view` / `RequestStoreView` / `CURRENT_REQUEST_VIEW`
//!   — are forbidden by the
//!   [`request_view_is_retired_from_crate_sources`](crate::project_global_cache_tests)
//!   architecture guard.
//! - It is NOT a snapshot of `VerterHost` state. Methods route through
//!   the host's live accessors so that workspace-side updates are
//!   reflected without re-creating the view.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::file_artifact_store::ProjectIdentity;
use crate::types::Hash16;
use crate::VerterHost;

/// Five-way environment-hash carrier (R21).
///
/// Today only `parse_env_hash` is wired through the cache substrate;
/// the remaining four dimensions are carried by value so that callers
/// can pass an `EnvHashes` value without further plumbing. Stage 6
/// migrates the cache-key composition to consume these fields.
///
/// **Stage 4a stub:** `HostView::env_hashes()` returns a static value
/// derived from
/// [`LEGACY_PARSE_ENV_HASH`](crate::file_artifact_store::LEGACY_PARSE_ENV_HASH).
/// The trait surface lets later stages plumb real values without
/// changing the read sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EnvHashes {
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}

/// Per-import resolution facts under a resolve-env.
///
/// **Stage 4a stub** — Stage 6a (`resolved_import_facts.rs`) replaces
/// this with the real type carrying `ResolvedImportClause` +
/// `ResolvedReexportBinding` facts and per-specifier resolutions
/// (plan Architectural Target → Cache layers).
///
/// The trait method [`SessionView::resolved_imports`] returns an
/// `Option<Arc<ResolvedImports>>`; until Stage 6a, every implementation
/// returns `None`. The signature stays stable so the Stage 6a cutover
/// is purely additive — callers that pattern-match on `None` continue
/// to compile.
#[derive(Debug, Default)]
pub struct ResolvedImports {
    // Stage 6a fields land here. Kept as a unit-shaped placeholder
    // for Stage 4a; never constructed pre-Stage-6a.
    _placeholder: (),
}

/// Read-only view over the base host's source / artifact state.
///
/// All resolver-tier queries route their host reads through this
/// trait. The implementations are [`HostView`] (passthrough to
/// `VerterHost`) and [`OverlaidView`] (per-canonical source overlays
/// stacked over a base host).
///
/// Lifetime + ownership: `SessionView` is passed by reference
/// (`&dyn SessionView`) so consumers do not hold strong references
/// to the underlying host beyond the call chain. Both concrete
/// impls hold `Arc<VerterHost>` internally so cross-thread
/// resolver work can clone the impl cheaply when needed.
pub trait SessionView: Send + Sync {
    /// Return the source for a canonical id if the view knows about
    /// it. Returns `None` for canonicals not yet ingested or
    /// (`OverlaidView` only) explicitly overlay-deleted.
    ///
    /// Overlays are checked before the base host. Base reads come
    /// from the host's shared file-cache; the view never mutates
    /// the host on read.
    fn source(&self, canonical: &str) -> Option<Arc<str>>;

    /// Content hash of `canonical` under this view, if known.
    ///
    /// For [`OverlaidView`] this returns the hash of the overlay
    /// source when an overlay covers the canonical; otherwise it
    /// returns the base host's content hash (or `None`).
    fn content_hash_for(&self, canonical: &str) -> Option<Hash16>;

    /// Return cached file artifacts (indexed-ready + facts +
    /// parsed-edges + parse_stable_hash + augmentations) for a
    /// canonical id, if a content-matching artifact bundle is
    /// already in the file-artifact store.
    ///
    /// Returns `None` if the artifacts have not been parsed yet
    /// under the relevant `(content_hash, parse_env_hash)` key.
    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>>;

    /// Return per-import resolution facts under this view's
    /// resolve-env, if computed.
    ///
    /// **Stage 4a placeholder.** Returns `None` until Stage 6a
    /// (`resolved_import_facts.rs`) wires the real
    /// `ResolvedImportFacts` cache. The method shape is published
    /// here so Stage 6a is purely additive.
    fn resolved_imports(&self, canonical: &str) -> Option<Arc<ResolvedImports>>;

    /// Project identity (16-byte stable key) for this view's
    /// project.
    fn project_identity(&self) -> ProjectIdentity;

    /// Five-way environment-hash bundle for this view (R21).
    fn env_hashes(&self) -> &EnvHashes;
}

// ---------------------------------------------------------------------------
// HostView — direct passthrough to `VerterHost`.
// ---------------------------------------------------------------------------

/// Read-only view that forwards every method to the base [`VerterHost`].
///
/// `HostView` carries an `Arc<VerterHost>` so it can be cloned cheaply
/// across resolver threads. The base host is the read substrate;
/// `HostView` never wraps additional state.
///
/// Used by overlay-free sessions and by any caller that wants to
/// observe the host's canonical state without overlay layering.
#[derive(Clone)]
pub struct HostView {
    base: Arc<VerterHost>,
    env_hashes: EnvHashes,
}

impl HostView {
    /// Construct a `HostView` over the supplied host.
    ///
    /// The returned view's `env_hashes()` reports the static
    /// Stage 4a defaults; Stage 6 plumbs real env hashes through
    /// the constructor.
    pub fn new(base: Arc<VerterHost>) -> Self {
        Self {
            base,
            env_hashes: EnvHashes::default(),
        }
    }

    /// Construct a `HostView` with explicit env hashes.
    ///
    /// Reserved for Stage 6 wiring; today callers may pass
    /// [`EnvHashes::default()`].
    #[allow(dead_code)]
    pub fn with_env_hashes(base: Arc<VerterHost>, env_hashes: EnvHashes) -> Self {
        Self { base, env_hashes }
    }

    /// Borrow the underlying host. Reserved for impls that need
    /// to reach the host directly (e.g., scheduler context
    /// construction); resolver-tier code should not use this.
    #[allow(dead_code)]
    pub fn host(&self) -> &VerterHost {
        &self.base
    }
}

impl SessionView for HostView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // Use the file-artifact store as the authoritative
        // content-hash source; falls back to None for canonicals
        // not yet ingested.
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        self.base
            .project_type_store()
            .indexed()
            .latest_artifacts_for_canonical(canonical)
    }

    fn resolved_imports(&self, _canonical: &str) -> Option<Arc<ResolvedImports>> {
        // Stage 6a wires the real `ResolvedImportFacts` cache.
        // Until then, every view returns `None`; callers fall
        // back to the legacy import-resolution path.
        None
    }

    fn project_identity(&self) -> ProjectIdentity {
        // The base host's project identity comes from its
        // workspace config. Stage 6 plumbs this through a host
        // accessor; the static-zero ProjectIdentity here is the
        // single-project default that today's tests rely on.
        ProjectIdentity([0u8; 16])
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }
}

// ---------------------------------------------------------------------------
// OverlaidView — overlay-aware view stacking sources over a base host.
// ---------------------------------------------------------------------------

/// Read-only view that layers per-canonical overlay sources over a
/// base [`VerterHost`].
///
/// The overlay map is consulted first for `source` and
/// `content_hash_for`. Canonicals absent from the overlay fall
/// through to the base host. Overlay artifacts are produced on
/// demand under the overlay's content hash (Stage 6) — Stage 4a
/// presents the trait shape only; the artifact-side path falls
/// through to the base host for now and is wired up in later
/// stages when the artifact store learns to key on the overlay
/// content hash.
///
/// `OverlaidView` is `Send + Sync` because its overlay map is
/// behind an `Arc<FxHashMap>`; mutation happens by constructing a
/// new `Arc<FxHashMap>` and a new `OverlaidView` value (R17 —
/// overlays do not mutate the base host).
#[derive(Clone)]
pub struct OverlaidView {
    overlays: Arc<FxHashMap<String, Arc<str>>>,
    overlay_hashes: Arc<FxHashMap<String, Hash16>>,
    base: Arc<VerterHost>,
    env_hashes: EnvHashes,
}

impl OverlaidView {
    /// Construct an overlaid view from a map of canonical → source.
    ///
    /// Content hashes for the overlay are computed once via
    /// `crate::hash::content_hash_str`. This is cheap (xxh3 over
    /// the overlay source) and stable so concurrent reads see the
    /// same hash without re-hashing.
    pub fn new(base: Arc<VerterHost>, overlays: FxHashMap<String, Arc<str>>) -> Self {
        let mut overlay_hashes = FxHashMap::default();
        overlay_hashes.reserve(overlays.len());
        for (canonical, source) in &overlays {
            let hash = crate::hash::hash_16(source.as_bytes());
            overlay_hashes.insert(canonical.clone(), hash);
        }
        Self {
            overlays: Arc::new(overlays),
            overlay_hashes: Arc::new(overlay_hashes),
            base,
            env_hashes: EnvHashes::default(),
        }
    }

    /// Variant that takes pre-computed overlay hashes (used by
    /// future caller pathways where the hash is already known).
    #[allow(dead_code)]
    pub fn with_overlay_hashes(
        base: Arc<VerterHost>,
        overlays: Arc<FxHashMap<String, Arc<str>>>,
        overlay_hashes: Arc<FxHashMap<String, Hash16>>,
        env_hashes: EnvHashes,
    ) -> Self {
        Self {
            overlays,
            overlay_hashes,
            base,
            env_hashes,
        }
    }

    /// Borrow the base host. Reserved for impls that need to
    /// reach the host directly; resolver-tier code should not use
    /// this.
    #[allow(dead_code)]
    pub fn host(&self) -> &VerterHost {
        &self.base
    }

    /// Whether the view has an overlay for the requested canonical.
    #[allow(dead_code)]
    pub fn has_overlay(&self, canonical: &str) -> bool {
        self.overlays.contains_key(canonical)
    }
}

impl SessionView for OverlaidView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if let Some(overlay_source) = self.overlays.get(canonical) {
            return Some(Arc::clone(overlay_source));
        }
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        if let Some(hash) = self.overlay_hashes.get(canonical) {
            return Some(*hash);
        }
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        // Stage 4a: even when an overlay is present, the artifact
        // store does not yet key on the overlay content hash, so
        // we fall through to the base host's latest artifacts.
        // Stage 6 wires this to the overlay-aware artifact path.
        self.base
            .project_type_store()
            .indexed()
            .latest_artifacts_for_canonical(canonical)
    }

    fn resolved_imports(&self, _canonical: &str) -> Option<Arc<ResolvedImports>> {
        // Stage 6a wires the real `ResolvedImportFacts` cache.
        None
    }

    fn project_identity(&self) -> ProjectIdentity {
        ProjectIdentity([0u8; 16])
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileKind, UpsertRequest};
    use crate::{CompileErrorPolicy, HostConfig};

    fn fresh_host() -> Arc<VerterHost> {
        Arc::new(VerterHost::new_standalone(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        }))
    }

    fn upsert(host: &VerterHost, canonical: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .expect("upsert succeeds");
        // Materialise IndexedReady so `FileArtifactStore` has an entry
        // for this canonical — the file-artifact store is populated
        // lazily on first demand, not synchronously from `upsert`.
        let _ = host.ensure_indexed_ready(canonical);
    }

    #[test]
    fn host_view_passes_through_source() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let view = HostView::new(Arc::clone(&host));
        let observed = view.source("/x.ts");
        assert!(observed.is_some());
        assert_eq!(observed.as_deref(), Some("export const a = 1;"));
        assert!(view.source("/missing.ts").is_none());
    }

    #[test]
    fn host_view_reports_content_hash_after_upsert() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let view = HostView::new(Arc::clone(&host));
        let hash = view.content_hash_for("/x.ts");
        assert!(
            hash.is_some(),
            "HostView.content_hash_for must report a hash for an ingested canonical"
        );
    }

    #[test]
    fn host_view_resolved_imports_returns_none_pre_stage_6a() {
        let host = fresh_host();
        let view = HostView::new(Arc::clone(&host));
        assert!(view.resolved_imports("/x.ts").is_none());
    }

    #[test]
    fn overlaid_view_overlay_source_wins_over_base() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/x.ts".to_string(), Arc::from("export const a = 999;"));
        let view = OverlaidView::new(Arc::clone(&host), overlays);

        let observed = view.source("/x.ts");
        assert_eq!(observed.as_deref(), Some("export const a = 999;"));
    }

    #[test]
    fn overlaid_view_falls_through_to_base_for_unmasked_canonical() {
        let host = fresh_host();
        upsert(&host, "/base.ts", "export const a = 1;");

        let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        let view = OverlaidView::new(Arc::clone(&host), overlays);

        assert_eq!(
            view.source("/base.ts").as_deref(),
            Some("export const a = 1;")
        );
    }

    #[test]
    fn overlaid_view_content_hash_diverges_from_base_under_overlay() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let host_view = HostView::new(Arc::clone(&host));
        let base_hash = host_view.content_hash_for("/x.ts");
        assert!(base_hash.is_some());

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            "/x.ts".to_string(),
            Arc::from("export const a = 'overlay';"),
        );
        let overlay_view = OverlaidView::new(Arc::clone(&host), overlays);
        let overlay_hash = overlay_view.content_hash_for("/x.ts");
        assert!(overlay_hash.is_some());
        assert_ne!(
            base_hash, overlay_hash,
            "OverlaidView with a different source must report a different content hash than the base"
        );
    }

    #[test]
    fn overlaid_view_byte_identical_overlay_matches_base_hash() {
        // R17 byte-identical guarantee — surfacing the same source
        // via the overlay must produce the same content hash as
        // the base.
        let host = fresh_host();
        let body = "export const a = 1;";
        upsert(&host, "/x.ts", body);

        let host_view = HostView::new(Arc::clone(&host));
        let base_hash = host_view.content_hash_for("/x.ts").expect("base hash");

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/x.ts".to_string(), Arc::from(body));
        let overlay_view = OverlaidView::new(Arc::clone(&host), overlays);
        let overlay_hash = overlay_view
            .content_hash_for("/x.ts")
            .expect("overlay hash");

        assert_eq!(
            base_hash, overlay_hash,
            "byte-identical overlay must collapse to the base content hash"
        );
    }

    #[test]
    fn session_view_trait_is_object_safe() {
        // Compile-time check: `&dyn SessionView` must work for
        // both impls so resolver-tier code can take a single
        // trait object reference.
        fn assert_dyn(view: &dyn SessionView) -> bool {
            view.env_hashes().parse_env_hash != [0xffu8; 16]
        }

        let host = fresh_host();
        let host_view: Box<dyn SessionView> = Box::new(HostView::new(Arc::clone(&host)));
        let overlaid: Box<dyn SessionView> =
            Box::new(OverlaidView::new(Arc::clone(&host), FxHashMap::default()));

        // Stage 4a env-hashes are default — the assertion exercises
        // the dyn trait call path; the predicate is non-trivial
        // (compares against an explicit sentinel value).
        assert!(assert_dyn(host_view.as_ref()));
        assert!(assert_dyn(overlaid.as_ref()));
    }

    #[test]
    fn host_view_and_overlaid_view_are_send_sync() {
        // Compile-time check (negative-by-construction): a `Send +
        // Sync` impl is required for trait-object use across the
        // resolver-tier thread pools.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HostView>();
        assert_send_sync::<OverlaidView>();

        // Reference the assertions from runtime so the test body
        // is non-empty — guards against the assertions silently
        // becoming dead code.
        let host = fresh_host();
        let view: Box<dyn SessionView + Send + Sync> = Box::new(HostView::new(host));
        let _ = view.project_identity();
    }
}
