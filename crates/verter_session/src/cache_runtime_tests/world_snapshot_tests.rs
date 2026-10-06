//! `WorldSnapshot::from_request` wiring through a live session host: the
//! constructor reads the store-view compat token through the resolver
//! context.

use crate::cache_runtime::world_snapshot::*;
use crate::types::HostConfig;
use crate::VerterHost;

#[test]
fn from_request_threads_dims_and_reads_compat_token_through_store_view() {
    // `from_request` takes `&dyn ResolverContext` and reads
    // `compat_token` through `(&crate::resolver_core::fact_validation_port::FactValidationView::new(ctx)).compat_token()`.
    // The inline test uses the compile-fenced direct-host fixture context.
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn crate::resolver_core::ResolverContext<crate::resolver_core::HostCapabilities> =
        &host;

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

    // `compat_token` reads through `(&crate::resolver_core::fact_validation_port::FactValidationView::new(ctx)).compat_token()`
    // — verify by re-reading directly and comparing.
    let expected_token = verter_session_query::facts::store_view::StoreView::compat_token(
        &crate::resolver_core::fact_validation_port::FactValidationView::new(ctx),
    );
    assert_eq!(
        snap.compat_token, expected_token,
        "from_request must read compat_token through (&crate::resolver_core::fact_validation_port::FactValidationView::new(ctx)).compat_token()",
    );
}

#[test]
fn dims_accessors_project_to_scoped_dimensions_through_from_request() {
    // Drive every `*_dims()` accessor through `from_request` so
    // production callers (B2+) cannot drop one and have the
    // missing accessor silently compile.
    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn crate::resolver_core::ResolverContext<crate::resolver_core::HostCapabilities> =
        &host;
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
