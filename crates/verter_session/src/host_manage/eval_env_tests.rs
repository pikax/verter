//! Discriminating tests for `host_manage::eval_env`.
//!
//! The per-file `EvalEnv` is not a stored `IndexedReady` field but the
//! lazy `whole_env()` demand product owned by `IndexedReady`'s
//! `DeclBodyMemo`, materialised on first demand and shared as one
//! `Arc`; `base_eval_env_arc` hands out that memo-owned whole-env.
//! Content-edit correctness comes from the content-addressed artifact
//! identity — no eager env-cache clear participates.

use std::sync::Arc;

/// Production-path discriminator — the per-file `EvalEnv` reflects a
/// content edit through artifact identity, not through an eager cache
/// clear.
///
/// The owner-upsert path has no eager reverse-dependent cascade. The
/// per-file env is the lazy `whole_env()` product of the
/// content-addressed `IndexedReady`'s `DeclBodyMemo`, not an
/// eagerly-stored field; the edited file's new content hash misses the
/// stale artifact and the materialise closure rebuilds the shallow
/// index from one fresh parse, so the next `whole_env()` demand lowers
/// the env and reflects the new declaration.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_eval_env_reflects_content_edit_without_eager_clear() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());

    // Initial content — one interface member.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Foo { a: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("initial upsert");

    // Build + cache the eval-env for the initial content.
    let env_before = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for initial content");
    assert!(
        env_before.type_declaration_id("Foo").is_some(),
        "precondition: initial eval-env knows interface Foo"
    );
    assert!(
        env_before.type_declaration_id("Bar").is_none(),
        "precondition: initial eval-env does NOT know Bar"
    );

    // Edit the file — add a second interface. The owner-upsert path
    // has no eager reverse-dependent cascade and no env cache to clear.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface Foo { a: number }\nexport interface Bar { b: string }",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("content edit upsert");

    // The eval-env for the edited file MUST reflect the new content.
    // The new content hash misses the stale `IndexedReady`, the
    // materialise closure rebuilds the shallow index from a fresh
    // parse, and the next `whole_env()` demand lowers the env from it.
    let env_after = host
        .base_eval_env("/src/types.ts")
        .expect("eval-env builds for edited content");
    assert!(
        env_after.type_declaration_id("Bar").is_some(),
        "the per-file eval-env MUST reflect a content edit via the \
         content-addressed IndexedReady identity. A missing `Bar` \
         here means a stale env was served."
    );
}

/// The env handed out by `base_eval_env_arc` IS the memo-owned whole-env
/// demand product — one shared `Arc`, no per-read rebuild.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_eval_env_arc_is_the_memo_owned_whole_env() {
    use crate::types::{FileLanguage, HostConfig, UpsertRequest};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Foo { a: number }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");

    let env = host
        .base_eval_env_arc("/src/types.ts")
        .expect("env must resolve");
    let indexed = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("artifact must exist");
    assert!(
        Arc::ptr_eq(&env, &indexed.shallow_state.decl_bodies().whole_env()),
        "base_eval_env_arc must hand out the IndexedReady-owned env Arc"
    );
    let env_again = host
        .base_eval_env_arc("/src/types.ts")
        .expect("env must resolve warm");
    assert!(
        Arc::ptr_eq(&env, &env_again),
        "repeated reads must share one env Arc"
    );
}
