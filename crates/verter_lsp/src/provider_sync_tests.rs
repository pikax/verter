//! Unit tests for [`crate::provider_sync`] provider-sync state transforms.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` in `provider_sync.rs` to
//! keep the production source under the file-size guard (`no_oversize_files`).
//! Wired back as a `#[cfg(test)] #[path = "provider_sync_tests.rs"] mod tests;`
//! child of `provider_sync`, so `use super::*` resolves to its items.

use super::*;

#[test]
fn vue_sync_state_uses_owner_key_from_tsconfig() {
    let resolver = NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace/pkg-a".to_string(),
            "/workspace".to_string(),
            Some("/workspace/pkg-a/tsconfig.json".to_string()),
        ),
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            None,
        ),
    ]);

    let state = vue_sync_state_for_source(&resolver, "/workspace/pkg-a/src/App.vue", false)
        .expect("matched Vue source should materialize provider state");

    assert_eq!(
        state.owner_binding,
        ProviderOwnerBinding::Owned("/workspace/pkg-a/tsconfig.json".to_string()),
        "owner_binding should be Owned with tsconfig path when available"
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/pkg-a/src/App.vue.tsx"),
        "provider IDE path should be canonical_id.tsx"
    );
    assert_eq!(
        state.api_path.as_deref(),
        Some("/workspace/pkg-a/src/App.vue.ts"),
        "Vue public API output should still be tracked alongside the IDE artifact"
    );
}

#[test]
fn current_owner_binding_reflects_resolver_owner_and_none() {
    let resolver =
        NativeProjectResolver::new(vec![crate::project_resolver::IdeProjectConfig::new(
            "/workspace/pkg-a".to_string(),
            "/workspace".to_string(),
            Some("/workspace/pkg-a/tsconfig.json".to_string()),
        )]);

    // Owned: a file under pkg-a resolves to its tsconfig key.
    let owned = current_owner_binding_for_source(&resolver, "/workspace/pkg-a/src/App.vue");
    assert_eq!(
        owned,
        ProviderOwnerBinding::Owned("/workspace/pkg-a/tsconfig.json".to_string()),
        "a file under a project must bind to that project's owner key"
    );

    // Unresolved: a file outside every project resolves to no owner.
    let unowned = current_owner_binding_for_source(&resolver, "/elsewhere/src/Other.vue");
    assert_eq!(
        unowned,
        ProviderOwnerBinding::Unresolved,
        "a file owned by no project must be Unresolved, got {unowned:?}"
    );
}

#[test]
fn committed_binding_matches_current_detects_owner_mismatch() {
    // R2-4: the skip-when-already-loaded gate must distinguish a binding
    // that still matches (safe to skip) from a changed/lost owner (must
    // reconcile).
    let owned_a = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/a/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };

    // Same owner → matches (safe to skip).
    assert!(
        committed_binding_matches_current(
            &owned_a,
            &ProviderOwnerBinding::Owned("/a/tsconfig.json".to_string())
        ),
        "an unchanged owner must still match"
    );
    // Owner changed → mismatch (must reconcile).
    assert!(
        !committed_binding_matches_current(
            &owned_a,
            &ProviderOwnerBinding::Owned("/b/tsconfig.json".to_string())
        ),
        "a changed owner must NOT match (force reconcile)"
    );
    // Owner lost (Owned → Unresolved) → mismatch (must reconcile). This is
    // the stranded-on-dead-owner case the skip-before-reconcile bug caused.
    assert!(
        !committed_binding_matches_current(&owned_a, &ProviderOwnerBinding::Unresolved),
        "owner loss (Owned→Unresolved) must NOT match (force reconcile)"
    );
    // Owner gained (Unresolved → Owned) → mismatch (must reconcile/upgrade).
    let unresolved = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    assert!(
        !committed_binding_matches_current(
            &unresolved,
            &ProviderOwnerBinding::Owned("/a/tsconfig.json".to_string())
        ),
        "owner gain (Unresolved→Owned) must NOT match (force upgrade)"
    );
}

#[test]
fn stale_paths_only_include_paths_that_change() {
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    let next = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };

    assert!(
        stale_paths_for_transition(&previous, &next).is_empty(),
        "same owner + same paths = no stale"
    );
}

#[test]
fn owner_change_forces_stale_even_when_paths_unchanged() {
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.old.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    let next = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.new.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };

    let stale = stale_paths_for_transition(&previous, &next);
    assert_eq!(
        stale.len(),
        2,
        "both active paths should be stale on owner change"
    );
    assert!(stale.contains(&(
        ProviderPathKind::Ide,
        "/workspace/src/App.vue.tsx".to_string()
    )));
    assert!(stale.contains(&(
        ProviderPathKind::Api,
        "/workspace/src/App.vue.ts".to_string()
    )));
}

#[test]
fn fallback_to_fallback_owner_change_detected() {
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/old-root".to_string()),
        shadow_path: Some("/workspace/src/utils.ts".to_string()),
        ..Default::default()
    };
    let next = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/new-root".to_string()),
        shadow_path: Some("/workspace/src/utils.ts".to_string()),
        ..Default::default()
    };

    let stale = stale_paths_for_transition(&previous, &next);
    assert_eq!(
        stale.len(),
        1,
        "fallback→fallback with different root = stale"
    );
    assert_eq!(stale[0].1, "/workspace/src/utils.ts");
}

#[test]
fn prepare_sync_transition_preserves_background_flags_for_unchanged_paths() {
    let states = DashMap::new();
    states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ide_background_loaded: true,
            api_background_loaded: true,
            ..Default::default()
        },
    );

    let transition = prepare_sync_transition(
        &states,
        "/workspace/src/App.vue",
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        },
    );

    assert!(transition.stale_paths.is_empty());
    assert!(transition.next.ide_background_loaded);
    assert!(transition.next.api_background_loaded);
}

#[test]
fn unresolved_state_is_detected() {
    let state = ProviderSyncState::unresolved("/workspace/src/App.vue.tsx".to_string());
    assert!(state.is_unresolved(), "unresolved state should be detected");
    assert_eq!(state.owner_binding, ProviderOwnerBinding::Unresolved);
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert!(state.api_path.is_none(), "unresolved has no API path");
}

#[test]
fn unresolved_vue_builds_local_tsx_ide_path() {
    let tsx = ProviderSyncState::unresolved_vue("/workspace/src/App.vue", false);
    assert!(tsx.is_unresolved());
    assert_eq!(tsx.ide_path.as_deref(), Some("/workspace/src/App.vue.tsx"));
    assert!(
        tsx.api_path.is_none(),
        "unresolved vue state has no API path"
    );

    // Negative: a JSX document must NOT get a `.tsx` path.
    let jsx = ProviderSyncState::unresolved_vue("/workspace/src/App.vue", true);
    assert_eq!(jsx.ide_path.as_deref(), Some("/workspace/src/App.vue.jsx"));
    assert_ne!(jsx.ide_path.as_deref(), Some("/workspace/src/App.vue.tsx"));
}

#[test]
fn unresolved_to_owner_aware_same_ide_path_not_stale() {
    let unresolved = ProviderSyncState::unresolved("/workspace/src/App.vue.tsx".to_string());
    let owner_aware = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };

    let stale = stale_paths_for_transition(&unresolved, &owner_aware);
    assert!(
        stale.is_empty(),
        "unresolved → owner-aware with same IDE path should not be stale, got: {:?}",
        stale
    );
}

#[test]
fn unresolved_to_owner_aware_different_ide_path_is_stale() {
    let unresolved = ProviderSyncState::unresolved("/workspace/src/App.vue.tsx".to_string());
    let owner_aware = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };

    let stale = stale_paths_for_transition(&unresolved, &owner_aware);
    assert_eq!(stale.len(), 1, "different IDE path should be stale");
    assert_eq!(stale[0].1, "/workspace/src/App.vue.tsx");
}

#[test]
fn remove_sync_state_returns_all_active_paths() {
    let states = DashMap::new();
    states.insert(
        "/workspace/src/util.ts".to_string(),
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace".to_string()),
            shadow_path: Some("/workspace/src/util.ts".to_string()),
            shadow_background_loaded: true,
            ..Default::default()
        },
    );

    let removed = remove_sync_state(&states, "/workspace/src/util.ts")
        .expect("source-keyed sync state should be removable");

    assert_eq!(
        removed.active_paths(),
        vec![(
            ProviderPathKind::Shadow,
            "/workspace/src/util.ts".to_string()
        )]
    );
    assert!(states.is_empty());
}

#[test]
fn open_unresolved_vue_state_converts_prior_owned_to_unresolved() {
    // FIX-1: an owned→unowned open Vue file must NOT keep its Owned binding
    // or its owner-derived `.vue.ts` API path; only the owner-independent
    // IDE TSX survives, and the binding is forced to Unresolved.
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };

    let state = open_unresolved_vue_state(Some(&previous), "/workspace/src/App.vue", false);

    // Discriminator: a naive "reuse prior state" would keep Owned + api_path.
    assert!(
        state.is_unresolved(),
        "owned→unowned open Vue state must become Unresolved, got {:?}",
        state.owner_binding
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the live IDE TSX path must be preserved"
    );
    assert!(
        state.ide_background_loaded,
        "the preserved IDE path keeps its background-loaded flag"
    );
    assert!(
        state.api_path.is_none(),
        "the stale owner-derived API path must be dropped, got {:?}",
        state.api_path
    );
    assert!(
        !state.api_background_loaded,
        "no API path means no API background-loaded flag"
    );
}

#[test]
fn open_unresolved_vue_state_builds_local_when_no_prior_ide_path() {
    // No prior state, or prior state without an IDE path → synthesize the
    // local `{src}.tsx`/`.jsx` unresolved Vue state.
    let none = open_unresolved_vue_state(None, "/workspace/src/App.vue", false);
    assert!(none.is_unresolved());
    assert_eq!(none.ide_path.as_deref(), Some("/workspace/src/App.vue.tsx"));

    let jsx = open_unresolved_vue_state(None, "/workspace/src/App.vue", true);
    assert_eq!(jsx.ide_path.as_deref(), Some("/workspace/src/App.vue.jsx"));

    // Prior state with no IDE path is treated as "no live path".
    let prior_no_ide = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    let rebuilt = open_unresolved_vue_state(Some(&prior_no_ide), "/workspace/src/App.vue", false);
    assert!(rebuilt.is_unresolved());
    assert_eq!(
        rebuilt.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert!(rebuilt.api_path.is_none());
}

#[test]
fn open_unresolved_vue_state_does_not_preserve_unloaded_prior_ide_path() {
    // R3-1: a committed unresolved open-document state may carry
    // `ide_background_loaded = true` ONLY for a path genuinely live in the
    // provider. A prior IDE path that was NEVER background-loaded
    // (`ide_background_loaded == false`) is a path the provider never opened
    // — it must NOT be carried forward as live, or `active_ide_path_for_uri`
    // would route hover/completion to an unopened ("dead") TSX. The result is
    // a freshly-targeted, not-yet-loaded path the caller must `open_tsx`
    // before committing it as live.
    //
    // Discriminator: a prior UNLOADED `.jsx` with `is_jsx == false`. Pre-fix
    // the impl reused the prior `.jsx` path verbatim (ignoring `is_jsx`); the
    // fix rebuilds the desired `.tsx` and never marks it live.
    let prev_unloaded_jsx = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: false, // never opened
        api_background_loaded: false,
        ..Default::default()
    };
    let state =
        open_unresolved_vue_state(Some(&prev_unloaded_jsx), "/workspace/src/App.vue", false);
    assert_ne!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "an unloaded prior `.jsx` must NOT be reused verbatim; the desired ext is `.tsx`"
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "the rebuilt target must be the desired-extension `.tsx`, got {:?}",
        state.ide_path
    );
    assert!(
        !state.ide_background_loaded,
        "an unloaded prior IDE path must not be advertised as background-loaded, got {state:?}"
    );
}

#[test]
fn open_unresolved_vue_state_uses_desired_ext_on_jsx_flip() {
    // R3-4: the desired unresolved IDE path is derived from the CURRENT
    // `is_jsx`. A prior live `.jsx` path must NOT be reused when the document
    // flipped to TS (`is_jsx == false`) — reusing it would sync the new TS
    // code into the wrong `.jsx` provider artifact. The prior path is
    // preserved only when it matches the desired extension AND is live.
    let prev_live_jsx = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: None,
        ide_background_loaded: true, // live .jsx
        ..Default::default()
    };
    // is_jsx flipped to false → desired path is `.tsx`.
    let flipped = open_unresolved_vue_state(Some(&prev_live_jsx), "/workspace/src/App.vue", false);
    assert_eq!(
        flipped.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "an is_jsx flip must target the desired `.tsx`, not reuse the prior `.jsx`, got {:?}",
        flipped.ide_path
    );
    // The freshly-targeted `.tsx` is NOT yet live (the caller must open it).
    assert!(
        !flipped.ide_background_loaded,
        "the freshly-targeted desired path is not yet background-loaded"
    );

    // Conversely, a prior live path that MATCHES the desired ext is preserved
    // as live (no churn, no needless re-open).
    let prev_live_tsx = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let kept = open_unresolved_vue_state(Some(&prev_live_tsx), "/workspace/src/App.vue", false);
    assert_eq!(
        kept.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a prior live path matching the desired ext is preserved"
    );
    assert!(
        kept.ide_background_loaded,
        "a preserved live matching path keeps its background-loaded flag"
    );
}

#[test]
fn dropped_api_path_on_unowned_conversion_returns_stale_owner_derived_api() {
    // R2-8: owned→unowned conversion drops the owner-derived `.vue.ts`. The
    // caller must close it, so the helper must surface it as a close target.
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let converted = open_unresolved_vue_state(Some(&previous), "/workspace/src/App.vue", false);

    let dropped = dropped_api_path_on_unowned_conversion(Some(&previous), &converted);
    assert_eq!(
        dropped,
        Some((
            ProviderPathKind::Api,
            "/workspace/src/App.vue.ts".to_string()
        )),
        "the dropped owner-derived API path must be surfaced for closing, got {dropped:?}"
    );

    // Discriminator: the IDE TSX is preserved, so it must NEVER be returned
    // as a close target (closing it would kill the open document's hover).
    assert!(
        !matches!(dropped, Some((ProviderPathKind::Ide, _))),
        "the live IDE TSX must never be a close target on owned→unowned"
    );
}

#[test]
fn dropped_api_path_on_unowned_conversion_none_when_no_api_or_unchanged() {
    // No previous state → nothing dropped.
    let converted = ProviderSyncState::unresolved("/workspace/src/App.vue.tsx".to_string());
    assert!(dropped_api_path_on_unowned_conversion(None, &converted).is_none());

    // Previous with no API path → nothing dropped.
    let prev_no_api = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: None,
        ..Default::default()
    };
    assert!(
        dropped_api_path_on_unowned_conversion(Some(&prev_no_api), &converted).is_none(),
        "a previous state without an API path drops nothing"
    );

    // Converted still carries the SAME API path → not dropped (no leak).
    let prev_with_api = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    let still_has_api = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    assert!(
        dropped_api_path_on_unowned_conversion(Some(&prev_with_api), &still_has_api).is_none(),
        "an unchanged API path is not dropped"
    );

    // An `Unresolved` prior state's API path is NOT owner-derived → not a
    // close target (the open-document editor-liveness path keeps it).
    let prev_unresolved_with_api = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    let converted_unresolved = open_unresolved_vue_state(
        Some(&prev_unresolved_with_api),
        "/workspace/src/App.vue",
        false,
    );
    assert!(
        dropped_api_path_on_unowned_conversion(
            Some(&prev_unresolved_with_api),
            &converted_unresolved
        )
        .is_none(),
        "an Unresolved prior binding has no owner-derived API path to close"
    );
}

#[test]
fn revert_unsynced_kinds_keeps_previous_path_for_failed_kind() {
    // FIX-2: prior ide=.jsx (live), api=.ts (live); new owner wants
    // ide=.tsx, api=.ts. IDE sync FAILED (Api synced). The committed state
    // must revert IDE to the previous live `.jsx` rather than advertise the
    // unsynced `.tsx`.
    let previous = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let mut committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: false,
        api_background_loaded: true,
        ..Default::default()
    };

    revert_unsynced_kinds(&mut committed, Some(&previous), &[ProviderPathKind::Api]);

    // Discriminator: without the revert, ide_path would stay on the unsynced `.tsx`.
    assert_eq!(
        committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "failed IDE kind must revert to the previous live path"
    );
    assert!(
        committed.ide_background_loaded,
        "reverted IDE path restores its loaded flag"
    );
    assert_eq!(
        committed.api_path.as_deref(),
        Some("/workspace/src/App.vue.ts"),
        "the synced API kind keeps its new path"
    );
    assert!(committed.api_background_loaded);
}

#[test]
fn revert_unsynced_kinds_clears_failed_kind_with_no_previous_path() {
    // A kind that failed AND had no previous path is cleared (never was live).
    let mut committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ..Default::default()
    };
    // No previous state, IDE failed, API synced.
    revert_unsynced_kinds(&mut committed, None, &[ProviderPathKind::Api]);
    assert!(
        committed.ide_path.is_none(),
        "failed kind with no prior path is cleared, got {:?}",
        committed.ide_path
    );
    assert!(!committed.ide_background_loaded);
    assert_eq!(
        committed.api_path.as_deref(),
        Some("/workspace/src/App.vue.ts")
    );
}

#[test]
fn genuinely_stale_after_sync_gates_on_kind_and_active() {
    // Stale .jsx (Ide) from a prior owner; committed reverted IDE back to
    // .jsx after IDE sync failed. Only Api synced.
    let committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let stale = vec![(
        ProviderPathKind::Ide,
        "/workspace/src/App.vue.jsx".to_string(),
    )];

    // IDE did NOT sync → its stale path must NOT be closed (per-kind gate),
    // and it is also still active after the revert (active gate).
    let to_close = genuinely_stale_after_sync(&stale, &committed, &[ProviderPathKind::Api]);
    assert!(
        to_close.is_empty(),
        "a stale path of an unsynced kind must not be closed, got {to_close:?}"
    );
}

#[test]
fn genuinely_stale_after_sync_kind_gate_isolated_from_active_filter() {
    // R2-7 isolation: exercise the per-kind gate INDEPENDENTLY of the active
    // filter. The stale IDE `.jsx` is NOT among the committed active paths
    // (the committed IDE path is `.tsx`), so the active filter alone would
    // NOT suppress it. Only the kind-gate (Ide ∉ synced_kinds) keeps it from
    // being closed — proving the kind gate works on its own. Without the
    // kind gate this would (wrongly) close a path whose kind never synced.
    let committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        // Committed IDE path is `.tsx` (NOT the stale `.jsx`), so the stale
        // `.jsx` is NOT active.
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let stale = vec![(
        ProviderPathKind::Ide,
        "/workspace/src/App.vue.jsx".to_string(),
    )];

    // Only Api synced; the stale `.jsx` is non-active yet its kind (Ide) did
    // NOT sync → the kind gate alone must suppress the close.
    let to_close = genuinely_stale_after_sync(&stale, &committed, &[ProviderPathKind::Api]);
    assert!(
        to_close.is_empty(),
        "the per-kind gate alone must suppress a non-active stale path whose kind \
         did not sync (active filter is not the reason here), got {to_close:?}"
    );

    // Control: the SAME non-active stale `.jsx`, but now its kind (Ide) DID
    // sync → it must be closed. This pins that the empty result above is the
    // kind gate, not a degenerate always-empty function.
    let to_close_when_kind_synced = genuinely_stale_after_sync(
        &stale,
        &committed,
        &[ProviderPathKind::Ide, ProviderPathKind::Api],
    );
    assert_eq!(
        to_close_when_kind_synced,
        vec![(
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.jsx".to_string()
        )],
        "when the kind DID sync, the non-active stale path is closed"
    );
}

#[test]
fn genuinely_stale_after_sync_closes_changed_synced_path() {
    // IDE genuinely changed .jsx→.tsx and the new .tsx DID sync; the old
    // .jsx is no longer active and its kind synced → close it.
    let committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let stale = vec![(
        ProviderPathKind::Ide,
        "/workspace/src/App.vue.jsx".to_string(),
    )];

    let to_close = genuinely_stale_after_sync(
        &stale,
        &committed,
        &[ProviderPathKind::Ide, ProviderPathKind::Api],
    );
    assert_eq!(
        to_close,
        vec![(
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.jsx".to_string()
        )],
        "a genuinely-stale path of a synced kind must be closed"
    );
}

#[test]
fn genuinely_stale_after_sync_skips_same_path_rebind() {
    // Owner change on the owner-INDEPENDENT IDE path: stale .tsx == committed
    // .tsx (same-path rebind). Even though the IDE kind synced, the active
    // filter must skip closing the live artifact.
    let committed = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/new".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let stale = vec![
        (
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.tsx".to_string(),
        ),
        (
            ProviderPathKind::Api,
            "/workspace/src/App.vue.ts".to_string(),
        ),
    ];

    let to_close = genuinely_stale_after_sync(
        &stale,
        &committed,
        &[ProviderPathKind::Ide, ProviderPathKind::Api],
    );
    assert!(
        to_close.is_empty(),
        "a same-path rebind must not close the live artifact, got {to_close:?}"
    );
}

// ---- open_unresolved_vue_commit decision table (R5-1) ----------------
//
// The pure state half of the unified unresolved-preserve liveness machine.
// One test per row of the brief's 9-row table. `P_old`/`L_old` = prior
// committed ide_path + loaded; `P_new` = desired-ext path; "prior live" =
// `P_old.is_some() && L_old`. Each test pins: committed `ide_path`, committed
// `ide_background_loaded` (so `active_ide_path_for_uri` returns it iff live),
// and the IDE close target (`stale_ide_after_success`).

/// Build the desired Unresolved target the caller passes to the commit
/// builder (mirrors `open_unresolved_vue_state(prev, src, is_jsx)`).
fn target_for(previous: Option<&ProviderSyncState>, is_jsx: bool) -> ProviderSyncState {
    open_unresolved_vue_state(previous, "/workspace/src/App.vue", is_jsx)
}

#[test]
fn open_unresolved_commit_row1_no_prior_no_ide() {
    // Row 1: no prior live state, IDE did not sync this pass → committed
    // ide_path None, not loaded, nothing to close.
    let target = target_for(None, false);
    let commit = open_unresolved_vue_commit(None, target, false);
    assert!(commit.committed.is_unresolved());
    assert!(
        commit.committed.ide_path.is_none(),
        "no prior + no sync must commit ide_path=None, got {:?}",
        commit.committed.ide_path
    );
    assert!(!commit.committed.ide_background_loaded);
    assert!(commit.dropped_api.is_none());
    assert!(commit.stale_ide_after_success.is_none());
}

#[test]
fn open_unresolved_commit_row2_no_prior_sync_ok() {
    // Row 2: no prior live state, IDE synced → committed P_new, loaded.
    let target = target_for(None, false);
    let commit = open_unresolved_vue_commit(None, target, true);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert!(
        commit.committed.ide_background_loaded,
        "a synced first-open must commit the path as live"
    );
    assert!(commit.stale_ide_after_success.is_none());
}

#[test]
fn open_unresolved_commit_row3_no_prior_sync_err() {
    // Row 3: no prior live state, IDE sync FAILED → committed None, not
    // loaded (the failed open never went live; no dead path advertised).
    let target = target_for(None, false);
    let commit = open_unresolved_vue_commit(None, target, false);
    assert!(
        commit.committed.ide_path.is_none(),
        "a failed first-open with no prior live path must commit ide_path=None, got {:?}",
        commit.committed.ide_path
    );
    assert!(!commit.committed.ide_background_loaded);
    assert!(commit.stale_ide_after_success.is_none());
}

#[test]
fn open_unresolved_commit_row4_live_same_ext_no_ide() {
    // Row 4: prior LIVE .tsx, same ext, IDE did NOT sync this pass → RETAIN
    // P_old, stays loaded, nothing closed.
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a prior live same-ext path is retained when IDE does not sync, got {:?}",
        commit.committed.ide_path
    );
    assert!(
        commit.committed.ide_background_loaded,
        "the retained prior live path keeps its loaded flag"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "a retained same-ext path must not be closed, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_row5_live_same_ext_sync_ok() {
    // Row 5: prior LIVE .tsx, same ext, IDE synced → P_new(=P_old), loaded,
    // no close (same-path rebind).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    let commit = open_unresolved_vue_commit(Some(&prev), target, true);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert!(commit.committed.ide_background_loaded);
    assert!(
        commit.stale_ide_after_success.is_none(),
        "a same-path rebind must not close the live artifact, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_row6_live_same_ext_sync_err() {
    // Row 6: prior LIVE .tsx, same ext, IDE UPDATE failed → RETAIN P_old,
    // stays loaded, nothing closed (stale content, but still the live path).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a failed in-place update retains the prior live path, got {:?}",
        commit.committed.ide_path
    );
    assert!(commit.committed.ide_background_loaded);
    assert!(commit.stale_ide_after_success.is_none());
}

#[test]
fn open_unresolved_commit_row7_live_diff_ext_no_ide() {
    // Row 7 (REGRESSION): prior LIVE .jsx, desired .tsx (is_jsx flip), IDE
    // did NOT sync this pass → RETAIN the prior live .jsx (still open in the
    // provider), loaded, .tsx queued, NEVER close the .jsx.
    //
    // Pre-unification `drop_unloaded_ide_path` dropped the freshly-rebuilt
    // .tsx (not loaded) AND committed ide_path=None while the .jsx stayed
    // physically open → active_ide_path_for_uri None → hover dies.
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false); // desired .tsx
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "a failed/absent flip must RETAIN the prior live .jsx (not drop to None), got {:?}",
        commit.committed.ide_path
    );
    assert!(
        commit.committed.ide_background_loaded,
        "the retained prior live .jsx keeps its loaded flag so it stays the active IDE path"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "the prior live .jsx must NEVER be closed when the flip did not sync, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_row8_live_diff_ext_sync_ok() {
    // Row 8: prior LIVE .jsx, desired .tsx, IDE synced → committed becomes
    // the new .tsx (P_new), loaded, and the old .jsx closes AFTER success.
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false); // desired .tsx
    let commit = open_unresolved_vue_commit(Some(&prev), target, true);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a successful flip commits the new .tsx"
    );
    assert!(commit.committed.ide_background_loaded);
    assert_eq!(
        commit.stale_ide_after_success,
        Some((
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.jsx".to_string()
        )),
        "the orphaned prior live .jsx is closed AFTER the new .tsx syncs, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_row9_live_diff_ext_sync_err() {
    // Row 9 (REGRESSION): prior LIVE .jsx, desired .tsx, IDE sync FAILED →
    // RETAIN the prior live .jsx (still open), loaded, NEVER close it, .tsx
    // queued.
    //
    // Pre-unification `drop_unloaded_ide_path` dropped the failed .tsx AND
    // committed ide_path=None while the .jsx stayed physically open → hover
    // dies (the exact P0 both codex reviewers flagged).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        ide_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false); // desired .tsx
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "a failed flip must RETAIN the prior live .jsx (not drop to None), got {:?}",
        commit.committed.ide_path
    );
    assert!(
        commit.committed.ide_background_loaded,
        "the retained prior live .jsx keeps its loaded flag"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "the prior live .jsx must NEVER be closed on a failed flip, got {:?}",
        commit.stale_ide_after_success
    );
}

// ---- prior-exists-but-UNLOADED rows (R6-1) ---------------------------
//
// The 9-row table above covers {no prior} and {prior LIVE} (`L_old == true`).
// It OMITS the {prior exists, `ide_background_loaded == false`} edge: a prior
// committed `ide_path = Some(p)` the provider never actually opened (L_old ==
// false). Only a prior LIVE path is ever a valid retain target, so an unloaded
// prior path must be treated as NO live prior: a non-syncing pass commits
// `ide_path = None` (never `Some(p)` with `loaded == false` — a path the
// provider has not opened). A successful sync this pass commits the new live
// path as usual. These rows use a SAME-extension prior path so the ONLY
// variable vs the live rows is `L_old`, isolating the bug to the loaded flag.

#[test]
fn open_unresolved_commit_prior_unloaded_no_ide() {
    // R6-1: prior exists with `ide_path = Some(.tsx)` but `L_old == false`
    // (never opened), IDE did NOT sync this pass → committed ide_path None,
    // not loaded, nothing to close.
    //
    // RED pre-fix: `prior_ide_only` carried `prev.ide_path` regardless of
    // `prev.ide_background_loaded`, so `revert_unsynced_kinds` reverted the
    // non-synced IDE kind to `Some(.tsx)` and committed a NON-LIVE path
    // (`ide_path = Some(.tsx)`, `ide_background_loaded = false`).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: false, // never opened in the provider
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert!(commit.committed.is_unresolved());
    assert!(
        commit.committed.ide_path.is_none(),
        "an UNLOADED prior path is not live, so a non-syncing pass must commit \
         ide_path=None (never Some(prev.path) with loaded=false), got {:?}",
        commit.committed.ide_path
    );
    assert!(
        !commit.committed.ide_background_loaded,
        "committed state must not advertise a path the provider never opened"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "an unloaded prior path was never open, so there is nothing to close, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_prior_unloaded_sync_err() {
    // R6-1: prior exists, `L_old == false`, IDE sync FAILED this pass →
    // committed ide_path None, not loaded (the failed open never went live;
    // the unloaded prior was never live either → no dead path advertised).
    //
    // RED pre-fix: identical to the no-ide case — `prior_ide_only` retained
    // the unloaded prior path and committed `Some(.tsx)` with loaded=false.
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: false, // never opened in the provider
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    // `ide_synced == false`: a fresh open of the desired path failed this pass.
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);
    assert!(
        commit.committed.ide_path.is_none(),
        "a failed open with only an UNLOADED (never-live) prior must commit \
         ide_path=None, got {:?}",
        commit.committed.ide_path
    );
    assert!(!commit.committed.ide_background_loaded);
    assert!(
        commit.stale_ide_after_success.is_none(),
        "nothing was live → nothing to close, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_prior_unloaded_sync_ok() {
    // R6-1: prior exists, `L_old == false`, IDE sync SUCCEEDED this pass →
    // committed becomes the freshly-synced live desired path. An unloaded
    // prior never being live, there is no stale prior path to close (the
    // desired path equals the prior path here, so it would be a same-path
    // rebind regardless). This row pins that the live-filter does NOT regress
    // the success case (a first successful open still commits the live path).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        ide_background_loaded: false, // never opened in the provider
        ..Default::default()
    };
    let target = target_for(Some(&prev), false);
    let commit = open_unresolved_vue_commit(Some(&prev), target, true);
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "a successful open commits the live desired path, got {:?}",
        commit.committed.ide_path
    );
    assert!(
        commit.committed.ide_background_loaded,
        "a synced open must commit the path as live"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "an unloaded prior path was never open → never a close target, got {:?}",
        commit.stale_ide_after_success
    );
}

#[test]
fn open_unresolved_commit_prior_owned_drops_and_surfaces_api_close() {
    // For every prior-Owned row: the owner-derived `.vue.ts` is dropped from
    // the committed state AND surfaced for an UNCONDITIONAL close, while the
    // owner-INDEPENDENT IDE path is preserved (never returned as the API
    // close target). Exercised on the row-9 shape (failed flip) to prove the
    // API drop+close is independent of the IDE outcome (R2-8).
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false); // desired .tsx, api None
    let commit = open_unresolved_vue_commit(Some(&prev), target, false);

    // Binding forced Unresolved, owner-derived API dropped from state.
    assert!(commit.committed.is_unresolved());
    assert!(
        commit.committed.api_path.is_none(),
        "the owner-derived API path must be dropped from the committed state, got {:?}",
        commit.committed.api_path
    );
    // The owner-derived API is surfaced for an unconditional close…
    assert_eq!(
        commit.dropped_api,
        Some((
            ProviderPathKind::Api,
            "/workspace/src/App.vue.ts".to_string()
        )),
        "the owner-derived .vue.ts must be surfaced for closing, got {:?}",
        commit.dropped_api
    );
    // …and the IDE path (prior live .jsx, retained on the failed flip) is
    // NEVER the API close target and is NOT closed as a stale IDE path.
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "the owner-independent prior live IDE path is retained on a failed flip"
    );
    assert!(
        !matches!(commit.dropped_api, Some((ProviderPathKind::Ide, _))),
        "the IDE TSX/JSX must never be the API close target"
    );
    assert!(
        commit.stale_ide_after_success.is_none(),
        "a retained prior live IDE path must not be a stale-close target on a failed flip"
    );
}

#[test]
fn open_unresolved_commit_prior_owned_diff_ext_sync_ok_drops_api_and_closes_old_ide() {
    // Prior Owned + successful flip: owner-derived API dropped+surfaced, AND
    // the orphaned prior live IDE path surfaced for close-after-success. The
    // IDE close is the ext-flip close (row 8), distinct from the API close.
    let prev = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/old/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
        api_path: Some("/workspace/src/App.vue.ts".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        ..Default::default()
    };
    let target = target_for(Some(&prev), false); // desired .tsx
    let commit = open_unresolved_vue_commit(Some(&prev), target, true);
    assert!(commit.committed.is_unresolved());
    assert!(commit.committed.api_path.is_none());
    assert_eq!(
        commit.dropped_api,
        Some((
            ProviderPathKind::Api,
            "/workspace/src/App.vue.ts".to_string()
        ))
    );
    assert_eq!(
        commit.committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert_eq!(
        commit.stale_ide_after_success,
        Some((
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.jsx".to_string()
        )),
        "the orphaned prior live .jsx is closed after the new .tsx syncs"
    );
}
