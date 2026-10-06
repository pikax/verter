//! The exact identity of one parsed file artifact.

use crate::analysis::types::Hash16;
use std::sync::Arc;
use verter_language::FileLanguage;

/// Cache key for [`FileArtifacts`].
///
/// Keys are content-addressed (R5, R6): identity is the conjunction of
/// `canonical`, `content_hash`, `parse_env_hash`, `parse_key`, and
/// `file_language_id`. Two project envs reading the same canonical at
/// the same `content_hash` but different `parse_env_hash` coexist; the
/// cache returns the matching entry for the caller's env.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileArtifactKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    /// Exact source-bytes, language, compatibility-domain, and syntax-profile identity.
    pub parse_key: verter_language::ParseKey,
    /// Session-private derived-artifact shape identity.
    pub build_toolchain_fingerprint: crate::source::toolchain::BuildToolchainFingerprint,
    /// The file's [`FileLanguage`] row — the PER-FILE classification
    /// dimension of artifact identity (R21 scoping: nothing
    /// capability-shaped enters the global `parse_env_hash`).
    ///
    /// Exact readers take this row from the scheduler/runtime source
    /// authority. Exact writers take the retained row from
    /// [`IndexedReady::file_language`]. Path classification is only a
    /// pre-runtime fallback for synthetic tests and genuinely overlay-only
    /// canonicals.
    pub file_language_id: FileLanguage,
}

impl FileArtifactKey {
    /// [`Self::for_source_identity`] for a plain script whose parse identity
    /// the source stage already derived: no pass over the source bytes.
    pub fn for_script_parse_identity(
        canonical: Arc<str>,
        content_hash: Hash16,
        parse_key: verter_language::ParseKey,
        file_language_id: FileLanguage,
        parse_env_hash: Hash16,
    ) -> Self {
        Self {
            canonical,
            content_hash,
            parse_env_hash,
            parse_key,
            build_toolchain_fingerprint:
                crate::source::toolchain::current_build_toolchain_fingerprint(),
            file_language_id,
        }
    }

    /// Extension-derived language for explicitly synthetic, source-less test
    /// keys. Production exact identity always comes from runtime authority.
    #[cfg(any(test, feature = "test-support"))]
    pub fn synthetic_file_language_for_test(canonical: &str) -> FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution()
    }

    /// Test-only constructor for a base-shaped synthetic key.
    ///
    /// A session-view overlay materialiser
    /// ([`crate::VerterHost::materialize_overlay_indexed_ready_with_view`])
    /// can resolve a relative import to an overlay-only helper that the
    /// base workspace cannot see — so the overlay's `IndexedReady`
    /// carries session-specific import routes. When the overlay source
    /// bytes are identical to the base file, the overlay's content hash
    /// equals the base hash, and a base-shaped key for the overlay
    /// would collide with the base artifact's key: a base read would
    /// observe the overlay's session routes, or the overlay read would
    /// silently get the base routes. Byte-identical overlays are the
    /// common case (every opened-but-unmodified file in an LSP session).
    ///
    /// Used by `tests/cases/g_misc0/eviction_policy.rs` and similar integration
    /// tests that need to construct multiple distinct
    /// `FileArtifactKey` variants for the same canonical to
    /// exercise the per-canonical retention sweep + the
    /// promotion-aware LRU floor. The production `pub(crate)`
    /// surface is unchanged; this `pub fn` exists only inside
    /// `#[cfg(any(test, feature = "test-support"))]` so production
    /// builds carry no public exposure of the base constructor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn base_for_test(canonical: Arc<str>, content_hash: Hash16) -> Self {
        let language = Self::synthetic_file_language_for_test(&canonical);
        let parse_key = verter_language::default_parse_identity_for("", &language)
            .expect("test canonical has a supported parse identity")
            .1;
        Self {
            canonical,
            content_hash,
            parse_env_hash: BASE_PARSE_ENV_HASH,
            parse_key,
            build_toolchain_fingerprint:
                crate::source::toolchain::current_build_toolchain_fingerprint(),
            file_language_id: language,
        }
    }

    /// Test-only constructor for an overlay-shaped synthetic key.
    #[cfg(any(test, feature = "test-support"))]
    pub fn overlay_scoped_for_test(
        canonical: Arc<str>,
        content_hash: Hash16,
        discriminator: Hash16,
    ) -> Self {
        let language = Self::synthetic_file_language_for_test(&canonical);
        let parse_key = verter_language::default_parse_identity_for("", &language)
            .expect("test canonical has a supported parse identity")
            .1;
        Self {
            canonical,
            content_hash,
            parse_env_hash: discriminator,
            parse_key,
            build_toolchain_fingerprint:
                crate::source::toolchain::current_build_toolchain_fingerprint(),
            file_language_id: language,
        }
    }

    /// `true` when this key has the base-artifact identity: the base
    /// parse-environment sentinel and current build-toolchain fingerprint.
    ///
    /// A non-base key carries a session-overlay **discriminator** in
    /// the `parse_env_hash` dimension — its
    /// `IndexedReady` can hold session-specific import routes resolved
    /// against an overlay-only helper the base workspace cannot see.
    ///
    /// The store's **base canonical-wide reads**
    /// ([`FileArtifactStore::get_any`], [`FileArtifactStore::get_artifacts_any`]
    /// — via their canonical→keys index candidates — and the
    /// [`FileArtifactStore::snapshot_all`] scan) filter on
    /// this predicate so a base `HostView` / `HostStoreView` reader can
    /// never observe an overlay-scoped artifact and derive base cache
    /// keys / route facts from another session's routes. A session-view
    /// reader reaches its overlay artifact through the exact-key /
    /// view-aware accessors ([`FileArtifactStore::get_overlay_scoped`],
    /// `OverlayArtifactIdentity::lookup_overlay_artifacts`) instead —
    /// they key on the full `FileArtifactKey` including the
    /// discriminator. Lifecycle / removal
    /// scans ([`FileArtifactStore::remove`],
    /// [`FileArtifactStore::remove_canonical`]) do NOT filter on this —
    /// an eviction must drain every key for a canonical, overlay-scoped
    /// keys included.
    #[must_use]
    pub fn is_base(&self) -> bool {
        self.parse_env_hash == BASE_PARSE_ENV_HASH
            && self.build_toolchain_fingerprint
                == crate::source::toolchain::current_build_toolchain_fingerprint()
    }
}

/// `parse_env_hash` sentinel marking a BASE artifact key
/// used by the canonical-keyed base surface
/// before later stages plumb the real env hash through every call site.
/// An overlay-scoped key carries a non-zero session discriminator in
/// this dimension instead, so it can never alias a base key.
pub const BASE_PARSE_ENV_HASH: Hash16 = [0u8; 16];
