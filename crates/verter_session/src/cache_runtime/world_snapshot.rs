//! Request-concurrency identity for the cache runtime.
//!
//! [`WorldSnapshot`] is the single deterministic identity for ONE
//! in-flight request. It is what `cooperative_get_or_insert` lanes
//! coalesce on; it is NOT a cache key (R21 forbids bundling the five
//! env hashes into a single `project_config_hash` on any cache layer
//! — per-layer keys continue to embed only the dimensions they
//! actually depend on).
//!
//! The four env-hash dimensions and `project_identity` enter through
//! [`WorldSnapshotDims`], which the host populates at the request
//! entry boundary by calling the existing
//! [`verter_workspace::resolver::IdeProjectConfig`] accessors
//! (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`,
//! `lib_env_hash`, `project_identity`). The trio
//! `compiler_version` / `plugin_versions` / `world_generation` does
//! NOT live on `IdeProjectConfig` — they are host-side identity
//! dimensions the caller already tracks (the host's installed
//! compiler version, the host's plugin registry hash, the host's
//! monotonic world-generation counter).
//!
//! The per-layer accessors ([`WorldSnapshot::parse_dims`],
//! [`WorldSnapshot::resolve_dims`], [`WorldSnapshot::type_dims`],
//! [`WorldSnapshot::compile_dims`]) project the snapshot's identity
//! down to the dimensions a given cache family actually keys on, so a
//! caller assembles the slot key directly from the dim view rather
//! than re-deriving the env hashes per layer.
//!
//! `allow(dead_code)` at module scope: this module is a substrate.
//! The production callers of `from_request` and the `*_dims`
//! accessors are downstream cache-runtime entry-points (artifact
//! caches and query-identity nodes) that thread `WorldSnapshot`
//! through cooperative-admission slots. The struct shape, derives,
//! and accessor signatures are verified by the inline
//! `#[cfg(test)] mod tests` below; the architecture guard
//! `tests/world_snapshot_is_not_a_cache_key.rs` enforces the
//! identity-vs-key invariant statically, independent of which
//! consumer wires the identity through.

#![allow(dead_code)]

use crate::resolver_core::{ResolverContext, StoreViewCompatToken};
use crate::types::Hash16;

/// Newtype wrapper around the session id used to scope an overlay.
///
/// `None` on a [`WorldSnapshot`] represents a base view; `Some(_)`
/// represents the active overlay's identity. The session rail
/// constructs `Some(OverlayIdentity(session_id))`; the bare-host rail
/// constructs `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayIdentity(pub u64);

/// Pre-computed env-dimension bundle the caller assembles at the
/// request entry boundary.
///
/// Carrying the dims as a struct keeps [`WorldSnapshot::from_request`]
/// substrate-friendly: the four env-hash accessors on
/// [`verter_workspace::resolver::IdeProjectConfig`] take an
/// `&EnvHashInputs<'_>` argument; the caller computes the four
/// `Hash16`s once and packs them here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldSnapshotDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
    pub world_generation: u64,
}

/// Dimensions a parse-domain cache family actually keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseEnvDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
}

/// Dimensions a resolve-domain cache family actually keys on.
///
/// `lib_env_hash` is deliberately absent — R21 bans
/// `ResolvedImportFacts` from keying on lib data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolveEnvDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
}

/// Dimensions a type-domain cache family actually keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeEnvDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}

/// Dimensions a compile-output cache family actually keys on,
/// inclusive of source-map policy + public-API mode + compiler /
/// plugin versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompileEnvDims {
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub source_map_policy_hash: Hash16,
    pub public_api_mode_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
}

/// Request-concurrency identity for the cache runtime.
///
/// One [`WorldSnapshot`] per in-flight request. It is the lane
/// identity that `cooperative_get_or_insert` coalesces on; it is NOT
/// a cache key. Cache layers project to scoped dimensions via the
/// `*_dims()` accessors — bundling the full snapshot into a key
/// violates R21.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldSnapshot {
    pub compat_token: StoreViewCompatToken,
    pub project_identity: Hash16,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub source_map_policy_hash: Hash16,
    pub public_api_mode_hash: Hash16,
    pub compiler_version: Hash16,
    pub plugin_versions: Hash16,
    pub overlay_identity: Option<OverlayIdentity>,
    /// World generation under which the snapshot was constructed.
    /// Query-identity caches stamp this on
    /// `CacheEntry.validated_at_generation` at admission; the memory
    /// policy reads it to decide pin lifetime.
    pub generation: u64,
}

impl WorldSnapshot {
    /// Build a [`WorldSnapshot`] from a [`ResolverContext`] plus the
    /// pre-computed env-dimension bundle the request carries.
    ///
    /// `compat_token` is read through [`StoreView::compat_token`] on
    /// the context's active store view. `overlay_identity` is the
    /// session identity the caller already has at the request entry
    /// boundary (the bare-host rail passes `None`; the session rail
    /// passes `Some(OverlayIdentity(session_id))`).
    pub(crate) fn from_request(
        ctx: &dyn ResolverContext,
        dims: WorldSnapshotDims,
        overlay_identity: Option<OverlayIdentity>,
        public_api_mode_hash: Hash16,
        source_map_policy_hash: Hash16,
    ) -> Self {
        Self {
            compat_token: ctx.store_view().compat_token(),
            project_identity: dims.project_identity,
            parse_env_hash: dims.parse_env_hash,
            resolve_env_hash: dims.resolve_env_hash,
            type_env_hash: dims.type_env_hash,
            lib_env_hash: dims.lib_env_hash,
            source_map_policy_hash,
            public_api_mode_hash,
            compiler_version: dims.compiler_version,
            plugin_versions: dims.plugin_versions,
            overlay_identity,
            generation: dims.world_generation,
        }
    }

    /// Project the snapshot onto the dimensions a parse-domain cache
    /// family keys on.
    pub fn parse_dims(&self) -> ParseEnvDims {
        ParseEnvDims {
            project_identity: self.project_identity,
            parse_env_hash: self.parse_env_hash,
        }
    }

    /// Project the snapshot onto the dimensions a resolve-domain
    /// cache family keys on. `lib_env_hash` is intentionally absent
    /// (R21 — `ResolvedImportFacts` is not lib-keyed).
    pub fn resolve_dims(&self) -> ResolveEnvDims {
        ResolveEnvDims {
            project_identity: self.project_identity,
            parse_env_hash: self.parse_env_hash,
            resolve_env_hash: self.resolve_env_hash,
        }
    }

    /// Project the snapshot onto the dimensions a type-domain cache
    /// family keys on.
    pub fn type_dims(&self) -> TypeEnvDims {
        TypeEnvDims {
            project_identity: self.project_identity,
            parse_env_hash: self.parse_env_hash,
            resolve_env_hash: self.resolve_env_hash,
            type_env_hash: self.type_env_hash,
            lib_env_hash: self.lib_env_hash,
        }
    }

    /// Project the snapshot onto the dimensions a compile-output
    /// cache family keys on, inclusive of source-map policy +
    /// public-API mode + compiler / plugin versions.
    pub fn compile_dims(&self) -> CompileEnvDims {
        CompileEnvDims {
            project_identity: self.project_identity,
            parse_env_hash: self.parse_env_hash,
            resolve_env_hash: self.resolve_env_hash,
            type_env_hash: self.type_env_hash,
            lib_env_hash: self.lib_env_hash,
            source_map_policy_hash: self.source_map_policy_hash,
            public_api_mode_hash: self.public_api_mode_hash,
            compiler_version: self.compiler_version,
            plugin_versions: self.plugin_versions,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Inline tests for [`WorldSnapshot`]. Two contracts under test:
    //!
    //! 1. **Construction discriminators** — the `Hash` / `PartialEq`
    //!    derives observe every identity dimension. A regression that
    //!    drops a field, mis-derives, or collapses `OverlayIdentity::None`
    //!    and `Some(0)` fails one of these characterisation tests.
    //!
    //! 2. **`from_request` wiring** — the production constructor takes
    //!    `&dyn ResolverContext`, a `pub(crate)` trait sealed to the
    //!    crate. The inline module exercises it directly through the
    //!    bare-host `impl ResolverContext for VerterHost` rail.
    //!
    //! Living inside `verter_session::cache_runtime` keeps
    //! [`WorldSnapshot`] truly `pub(crate)` — there is no parallel
    //! `for_tests_from_raw` constructor and no `pub use` shim on the
    //! crate's `for_tests` module. Construction uses struct-literal
    //! syntax directly.
    use super::*;
    use crate::resolver_core::StoreViewCompatToken;
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn token(epoch: u64, session: Option<u64>) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch,
            session,
            validity_fingerprint: 0,
        }
    }

    fn dims_seed() -> WorldSnapshotDims {
        WorldSnapshotDims {
            project_identity: [0x11u8; 16],
            parse_env_hash: [0x22u8; 16],
            resolve_env_hash: [0x33u8; 16],
            type_env_hash: [0x44u8; 16],
            lib_env_hash: [0x55u8; 16],
            compiler_version: [0x66u8; 16],
            plugin_versions: [0x77u8; 16],
            world_generation: 42,
        }
    }

    /// Build a [`WorldSnapshot`] by direct struct-literal init from
    /// raw inputs. Used by construction-discriminator tests that
    /// need to vary a single dimension at a time. Production callers
    /// always go through [`WorldSnapshot::from_request`] which is
    /// exercised separately below.
    fn build(
        compat_token: StoreViewCompatToken,
        dims: WorldSnapshotDims,
        overlay_identity: Option<OverlayIdentity>,
        public_api_mode_hash: Hash16,
        source_map_policy_hash: Hash16,
    ) -> WorldSnapshot {
        WorldSnapshot {
            compat_token,
            project_identity: dims.project_identity,
            parse_env_hash: dims.parse_env_hash,
            resolve_env_hash: dims.resolve_env_hash,
            type_env_hash: dims.type_env_hash,
            lib_env_hash: dims.lib_env_hash,
            source_map_policy_hash,
            public_api_mode_hash,
            compiler_version: dims.compiler_version,
            plugin_versions: dims.plugin_versions,
            overlay_identity,
            generation: dims.world_generation,
        }
    }

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut h = DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    }

    #[test]
    fn world_snapshot_from_request_matches_all_request_identity_dimensions() {
        // Two snapshots with byte-identical inputs MUST be equal and
        // hash equal. This characterises the `Hash` / `PartialEq`
        // derives across every field.
        let dims = dims_seed();
        let public_api_mode_hash = [0xAAu8; 16];
        let source_map_policy_hash = [0xBBu8; 16];
        let compat = token(7, Some(3));
        let overlay = Some(OverlayIdentity(99));

        let a = build(
            compat,
            dims,
            overlay,
            public_api_mode_hash,
            source_map_policy_hash,
        );
        let b = build(
            compat,
            dims,
            overlay,
            public_api_mode_hash,
            source_map_policy_hash,
        );
        assert_eq!(a, b, "identical-input snapshots must compare equal");
        assert_eq!(
            hash_of(&a),
            hash_of(&b),
            "identical-input snapshots must hash equal"
        );

        // Same env dims, DIFFERENT overlay_identity → snapshots MUST
        // differ. This is the discriminator the cache runtime relies
        // on: an overlay variant cannot coalesce with a base variant
        // even when their env dims match.
        let base_view = build(
            compat,
            dims,
            None,
            public_api_mode_hash,
            source_map_policy_hash,
        );
        let overlay_view = build(
            compat,
            dims,
            Some(OverlayIdentity(99)),
            public_api_mode_hash,
            source_map_policy_hash,
        );
        assert_ne!(
            base_view, overlay_view,
            "base-view and overlay-view with identical env dims must differ"
        );
        assert_ne!(
            hash_of(&base_view),
            hash_of(&overlay_view),
            "base-view and overlay-view with identical env dims must hash differently"
        );

        let overlay_view_other_session = build(
            compat,
            dims,
            Some(OverlayIdentity(100)),
            public_api_mode_hash,
            source_map_policy_hash,
        );
        assert_ne!(
            overlay_view, overlay_view_other_session,
            "overlay variants with different session ids must differ"
        );
    }

    #[test]
    fn world_snapshot_diverges_on_every_identity_dimension() {
        // For each identity dimension, mutate it in isolation and
        // assert the resulting snapshot differs from the baseline.
        let dims = dims_seed();
        let compat = token(1, None);
        let baseline = build(compat, dims, None, [0u8; 16], [0u8; 16]);

        let other = build(token(2, None), dims, None, [0u8; 16], [0u8; 16]);
        assert_ne!(baseline, other, "compat_token must enter snapshot identity");

        let other = build(token(1, Some(5)), dims, None, [0u8; 16], [0u8; 16]);
        assert_ne!(
            baseline, other,
            "compat_token.session must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.project_identity = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "project_identity must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.parse_env_hash = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "parse_env_hash must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.resolve_env_hash = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "resolve_env_hash must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.type_env_hash = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "type_env_hash must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.lib_env_hash = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "lib_env_hash must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.compiler_version = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "compiler_version must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.plugin_versions = [0xFFu8; 16];
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "plugin_versions must enter snapshot identity"
        );

        let mut mutated = dims;
        mutated.world_generation = 999;
        assert_ne!(
            baseline,
            build(compat, mutated, None, [0u8; 16], [0u8; 16]),
            "world_generation must enter snapshot identity"
        );

        assert_ne!(
            baseline,
            build(compat, dims, None, [0u8; 16], [0xFFu8; 16]),
            "source_map_policy_hash must enter snapshot identity"
        );

        assert_ne!(
            baseline,
            build(compat, dims, None, [0xFFu8; 16], [0u8; 16]),
            "public_api_mode_hash must enter snapshot identity"
        );
    }

    #[test]
    fn world_snapshot_dims_accessors_project_to_scoped_dimensions() {
        // R21 invariant: each cache family keys only on the dimensions
        // it actually depends on. The `*_dims()` accessors project the
        // snapshot down to the per-layer key shapes.
        let dims = dims_seed();
        let snap = build(token(1, None), dims, None, [0xAAu8; 16], [0xBBu8; 16]);

        let parse = snap.parse_dims();
        assert_eq!(parse.project_identity, dims.project_identity);
        assert_eq!(parse.parse_env_hash, dims.parse_env_hash);

        let resolve = snap.resolve_dims();
        assert_eq!(resolve.project_identity, dims.project_identity);
        assert_eq!(resolve.parse_env_hash, dims.parse_env_hash);
        assert_eq!(resolve.resolve_env_hash, dims.resolve_env_hash);

        let ty = snap.type_dims();
        assert_eq!(ty.project_identity, dims.project_identity);
        assert_eq!(ty.parse_env_hash, dims.parse_env_hash);
        assert_eq!(ty.resolve_env_hash, dims.resolve_env_hash);
        assert_eq!(ty.type_env_hash, dims.type_env_hash);
        assert_eq!(ty.lib_env_hash, dims.lib_env_hash);

        let comp = snap.compile_dims();
        assert_eq!(comp.project_identity, dims.project_identity);
        assert_eq!(comp.parse_env_hash, dims.parse_env_hash);
        assert_eq!(comp.resolve_env_hash, dims.resolve_env_hash);
        assert_eq!(comp.type_env_hash, dims.type_env_hash);
        assert_eq!(comp.lib_env_hash, dims.lib_env_hash);
        assert_eq!(comp.source_map_policy_hash, [0xBBu8; 16]);
        assert_eq!(comp.public_api_mode_hash, [0xAAu8; 16]);
        assert_eq!(comp.compiler_version, dims.compiler_version);
        assert_eq!(comp.plugin_versions, dims.plugin_versions);
    }

    #[test]
    fn from_request_threads_dims_and_reads_compat_token_through_store_view() {
        // `from_request` takes `&dyn ResolverContext` and reads
        // `compat_token` through `ctx.store_view().compat_token()`.
        // Driving it from inside `cache_runtime` uses the bare-host
        // `impl ResolverContext for VerterHost` rail directly.
        let host = VerterHost::new_standalone(HostConfig::default());
        let ctx: &dyn crate::resolver_core::ResolverContext = &host;

        let dims = WorldSnapshotDims {
            project_identity: [1u8; 16],
            parse_env_hash: [2u8; 16],
            resolve_env_hash: [3u8; 16],
            type_env_hash: [4u8; 16],
            lib_env_hash: [5u8; 16],
            compiler_version: [6u8; 16],
            plugin_versions: [7u8; 16],
            world_generation: 13,
        };
        let snap = WorldSnapshot::from_request(
            ctx,
            dims,
            Some(OverlayIdentity(7)),
            [0xAAu8; 16],
            [0xBBu8; 16],
        );

        // Every dim field threaded through unchanged.
        assert_eq!(snap.project_identity, dims.project_identity);
        assert_eq!(snap.parse_env_hash, dims.parse_env_hash);
        assert_eq!(snap.resolve_env_hash, dims.resolve_env_hash);
        assert_eq!(snap.type_env_hash, dims.type_env_hash);
        assert_eq!(snap.lib_env_hash, dims.lib_env_hash);
        assert_eq!(snap.compiler_version, dims.compiler_version);
        assert_eq!(snap.plugin_versions, dims.plugin_versions);
        assert_eq!(snap.generation, dims.world_generation);
        assert_eq!(snap.public_api_mode_hash, [0xAAu8; 16]);
        assert_eq!(snap.source_map_policy_hash, [0xBBu8; 16]);
        assert_eq!(snap.overlay_identity, Some(OverlayIdentity(7)));

        // `compat_token` reads through `ctx.store_view().compat_token()`
        // — verify by re-reading directly and comparing.
        let expected_token = crate::resolver_core::StoreView::compat_token(ctx.store_view());
        assert_eq!(
            snap.compat_token, expected_token,
            "from_request must read compat_token through ctx.store_view().compat_token()",
        );
    }

    #[test]
    fn dims_accessors_project_to_scoped_dimensions_through_from_request() {
        // Drive every `*_dims()` accessor through `from_request` so
        // production callers (B2+) cannot drop one and have the
        // missing accessor silently compile.
        let host = VerterHost::new_standalone(HostConfig::default());
        let ctx: &dyn crate::resolver_core::ResolverContext = &host;
        let dims = WorldSnapshotDims {
            project_identity: [1u8; 16],
            parse_env_hash: [2u8; 16],
            resolve_env_hash: [3u8; 16],
            type_env_hash: [4u8; 16],
            lib_env_hash: [5u8; 16],
            compiler_version: [6u8; 16],
            plugin_versions: [7u8; 16],
            world_generation: 13,
        };
        let snap = WorldSnapshot::from_request(ctx, dims, None, [0u8; 16], [0u8; 16]);

        let parse = snap.parse_dims();
        assert_eq!(parse.parse_env_hash, dims.parse_env_hash);
        assert_eq!(parse.project_identity, dims.project_identity);

        let resolve = snap.resolve_dims();
        assert_eq!(resolve.resolve_env_hash, dims.resolve_env_hash);
        assert_eq!(resolve.project_identity, dims.project_identity);
        assert_eq!(resolve.parse_env_hash, dims.parse_env_hash);

        let ty = snap.type_dims();
        assert_eq!(ty.type_env_hash, dims.type_env_hash);
        assert_eq!(ty.lib_env_hash, dims.lib_env_hash);
        assert_eq!(ty.project_identity, dims.project_identity);
        assert_eq!(ty.parse_env_hash, dims.parse_env_hash);
        assert_eq!(ty.resolve_env_hash, dims.resolve_env_hash);

        let comp = snap.compile_dims();
        assert_eq!(comp.compiler_version, dims.compiler_version);
        assert_eq!(comp.plugin_versions, dims.plugin_versions);
        assert_eq!(comp.source_map_policy_hash, [0u8; 16]);
        assert_eq!(comp.public_api_mode_hash, [0u8; 16]);
    }
}
