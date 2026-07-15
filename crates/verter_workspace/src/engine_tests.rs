//! Workspace-default env-hash caching tests.
//!
//! The workspace-default env-hash array is a pure function of the engine's
//! `default_resolve_extensions` list (every other input is a workspace
//! constant), and the workspace-default project identity is a process-wide
//! constant. The engine caches both so per-store-view reads
//! (`host_view_project_identity` / `host_view_env_hashes_for` no-owner
//! fallback on the session side) stop re-running the full
//! `IdeProjectConfig::new` → membership-glob-compile → 4×hash pipeline.
//!
//! These tests pin: cached values are byte-equal to an uncached fresh
//! computation; an extension-list republish invalidates the cached array;
//! readers racing a concurrent extension change only ever observe a value
//! derived from one published extension list (never a torn mix).

use std::sync::Arc;

use super::{
    compute_workspace_default_env_hash_array, workspace_default_env_hash_array_for_engine,
    workspace_default_project_identity_hash_for_engine, Engine,
};
use crate::published_state::ProjectEnvHashArray;
use crate::resolver::IdeProjectConfig;
use crate::traits::WorkspaceAccess;

/// Uncached reference computation from the engine's LIVE extension list —
/// the exact semantics the cached read path must preserve.
fn fresh_default_env_hash_array(engine: &Engine) -> ProjectEnvHashArray {
    compute_workspace_default_env_hash_array(&engine.default_resolve_extensions.load_full())
}

#[test]
fn cached_default_env_hash_array_equals_fresh_computation() {
    let engine = Engine::new();
    let fresh = fresh_default_env_hash_array(&engine);

    let cold = workspace_default_env_hash_array_for_engine(&engine);
    let warm = workspace_default_env_hash_array_for_engine(&engine);

    assert_eq!(cold, fresh, "cold read must equal uncached computation");
    assert_eq!(
        warm, fresh,
        "warm (cached) read must equal uncached computation"
    );
    assert_ne!(
        cold, [[0u8; 16]; 4],
        "workspace default is deliberately non-zero (distinct from the all-zero trait fallback)"
    );
}

#[test]
fn cached_default_project_identity_equals_fresh_computation() {
    let engine = Engine::new();
    let fresh = IdeProjectConfig::new(String::new(), String::new(), None).project_identity();

    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&engine),
        fresh,
        "cold read must equal uncached computation"
    );
    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&engine),
        fresh,
        "warm (cached) read must equal uncached computation"
    );
    // Engine-independent constant: a second engine observes the same value.
    let other = Engine::new();
    assert_eq!(
        workspace_default_project_identity_hash_for_engine(&other),
        fresh
    );
    assert_ne!(
        fresh, [0u8; 16],
        "default identity must not collapse to all-zero"
    );
}

#[test]
fn extension_republish_invalidates_cached_default_env_hash_array() {
    let engine = Engine::new();
    let before = workspace_default_env_hash_array_for_engine(&engine);

    // Novel extension (not in `probe_extensions()`) — the merged list changes.
    engine.set_default_resolve_extensions(vec![".verterext".to_string()]);

    let after = workspace_default_env_hash_array_for_engine(&engine);
    assert_eq!(
        after,
        fresh_default_env_hash_array(&engine),
        "post-republish read must equal uncached computation over the NEW list"
    );

    // Extensions feed exactly the resolve dimension (R21): parse/type/lib
    // are extension-independent, resolve must move.
    assert_eq!(
        before[0], after[0],
        "parse_env_hash must not depend on extensions"
    );
    assert_ne!(
        before[1], after[1],
        "resolve_env_hash must change with the extension list"
    );
    assert_eq!(
        before[2], after[2],
        "type_env_hash must not depend on extensions"
    );
    assert_eq!(
        before[3], after[3],
        "lib_env_hash must not depend on extensions"
    );

    // Republishing the SAME list is value-stable.
    engine.set_default_resolve_extensions(vec![".verterext".to_string()]);
    assert_eq!(workspace_default_env_hash_array_for_engine(&engine), after);
}

#[test]
fn memory_workspace_trait_surface_tracks_extension_republish() {
    let ws = crate::memory::MemoryWorkspace::new(crate::memory::MemoryOptions::default());
    let before = ws.workspace_default_env_hash_array();
    assert_eq!(before, fresh_default_env_hash_array(&ws.engine));

    ws.set_default_resolve_extensions(vec![".verterext".to_string()]);

    let after = ws.workspace_default_env_hash_array();
    assert_eq!(after, fresh_default_env_hash_array(&ws.engine));
    assert_ne!(before, after, "trait surface must observe the invalidation");

    // Identity is extension-independent and stable across the republish.
    assert_eq!(
        ws.workspace_default_project_identity_hash(),
        IdeProjectConfig::new(String::new(), String::new(), None).project_identity()
    );
}

#[test]
fn concurrent_readers_racing_extension_change_observe_only_published_values() {
    let engine = Arc::new(Engine::new());
    let expected_old = fresh_default_env_hash_array(&engine);

    // Deterministic expected NEW value: an identical engine with the same
    // republish produces the same merged list, hence the same array.
    let reference = Engine::new();
    reference.set_default_resolve_extensions(vec![".verterext".to_string()]);
    let expected_new = fresh_default_env_hash_array(&reference);
    assert_ne!(expected_old, expected_new);

    let writer = {
        let engine = Arc::clone(&engine);
        std::thread::spawn(move || {
            engine.set_default_resolve_extensions(vec![".verterext".to_string()]);
        })
    };
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let observed = workspace_default_env_hash_array_for_engine(&engine);
                    assert!(
                        observed == expected_old || observed == expected_new,
                        "observed a value not derived from any published extension list"
                    );
                }
            })
        })
        .collect();

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }
    // Post-quiescence: cache settles on the new published list.
    assert_eq!(
        workspace_default_env_hash_array_for_engine(&engine),
        expected_new
    );
}
