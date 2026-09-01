//! Coverage for [`SnapshotOwnerAuthority`] — the configured-owner decision a
//! provider stamps its per-file `projectRootPath` from.
//!
//! Wired back as `#[cfg(test)] #[path = "configured_owner_tests.rs"] mod tests;`
//! from `configured_owner.rs`.
//!
//! Discrimination: every assertion below is about a NESTED package inside a
//! single-folder workspace — the exact layout a folder-derived project root
//! gets wrong. An authority that answered the workspace folder (or that
//! answered the snapshot's decision only for source files, leaving generated
//! companions folder-rooted) fails these.

use super::*;

use verter_semantic::resolver_core::ModuleResolverCore;

/// The owner's root, or `None` for the terminal `NoProject` answer.
fn owned_root(ownership: &ProjectOwnership) -> Option<&str> {
    match ownership {
        ProjectOwnership::Owned(owner) => Some(owner.root.as_str()),
        ProjectOwnership::NoProject => None,
    }
}

/// The owner's defining config, or `None` for the terminal `NoProject` answer.
fn owned_config(ownership: &ProjectOwnership) -> Option<&str> {
    match ownership {
        ProjectOwnership::Owned(owner) => Some(owner.config_path.as_str()),
        ProjectOwnership::NoProject => None,
    }
}

/// A single-folder pnpm monorepo: the workspace root is a configured project,
/// and `packages/app` is its own configured project with its own install.
fn monorepo_authority() -> SnapshotOwnerAuthority {
    let resolver = ModuleResolverCore::new(vec![
        verter_workspace::ide_project_config(
            "/ws".to_string(),
            "/ws".to_string(),
            Some("/ws/tsconfig.json".to_string()),
        ),
        verter_workspace::ide_project_config(
            "/ws/packages/app".to_string(),
            "/ws".to_string(),
            Some("/ws/packages/app/tsconfig.json".to_string()),
        ),
    ]);
    let snapshot = crate::test_utils::make_test_snapshot(
        resolver,
        &[
            ("/ws", "/ws", Some("/ws/tsconfig.json")),
            (
                "/ws/packages/app",
                "/ws",
                Some("/ws/packages/app/tsconfig.json"),
            ),
        ],
    );
    SnapshotOwnerAuthority::new(snapshot)
}

#[test]
fn nested_package_source_binds_to_the_package_project_not_the_workspace_folder() {
    let authority = monorepo_authority();
    assert_eq!(
        owned_root(&authority.configured_owner("/ws/packages/app/src/App.vue")),
        Some("/ws/packages/app"),
    );
}

#[test]
fn carrier_sources_and_javascript_companions_bind_to_their_exact_nested_project() {
    let authority = monorepo_authority();
    for path in [
        "/ws/packages/app/src/App.vue",
        "/ws/packages/app/src/App.vue.jsx",
        "/ws/packages/app/src/App.svelte",
        "/ws/packages/app/src/App.svelte.jsx",
    ] {
        let ownership = authority.configured_owner(path);
        assert_eq!(
            owned_root(&ownership),
            Some("/ws/packages/app"),
            "{path} must bind through its authored source to the nested project root"
        );
        assert_eq!(
            owned_config(&ownership),
            Some("/ws/packages/app/tsconfig.json"),
            "{path} must retain the exact nested project config"
        );
    }
}

#[test]
fn a_file_outside_every_nested_package_still_binds_to_the_root_project() {
    let authority = monorepo_authority();
    assert_eq!(
        owned_root(&authority.configured_owner("/ws/src/Root.vue")),
        Some("/ws"),
    );
}

#[test]
fn the_owner_carries_the_exact_config_file_that_defines_the_project() {
    // The root is where the project's `node_modules` live; the CONFIG is what
    // gives it compiler options and identity. A directory can hold several
    // configured projects, so the root can never imply the config.
    let authority = monorepo_authority();
    assert_eq!(
        authority.configured_owner("/ws/packages/app/src/App.vue"),
        ProjectOwnership::Owned(verter_type_runtime::traits::ConfiguredOwner {
            root: "/ws/packages/app".to_string(),
            config_path: "/ws/packages/app/tsconfig.json".to_string(),
        }),
    );
    // …and the ROOT project's own config, not the nested one, for a file it owns.
    assert_eq!(
        owned_config(&authority.configured_owner("/ws/src/Root.vue")),
        Some("/ws/tsconfig.json"),
    );
}

#[test]
fn a_project_configured_by_jsconfig_is_identified_by_that_file() {
    // `jsconfig.json` configures a project exactly like `tsconfig.json`. An
    // authority that reported the root and let the consumer look for a literal
    // `tsconfig.json` would leave this project with invented default options.
    let resolver = ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
        "/ws/packages/legacy".to_string(),
        "/ws".to_string(),
        Some("/ws/packages/legacy/jsconfig.json".to_string()),
    )]);
    let snapshot = crate::test_utils::make_test_snapshot(
        resolver,
        &[(
            "/ws/packages/legacy",
            "/ws",
            Some("/ws/packages/legacy/jsconfig.json"),
        )],
    );
    let authority = SnapshotOwnerAuthority::new(snapshot);
    assert_eq!(
        owned_config(&authority.configured_owner("/ws/packages/legacy/src/main.js")),
        Some("/ws/packages/legacy/jsconfig.json"),
    );
    for path in [
        "/ws/packages/legacy/src/App.vue",
        "/ws/packages/legacy/src/App.vue.jsx",
        "/ws/packages/legacy/src/App.svelte",
        "/ws/packages/legacy/src/App.svelte.jsx",
    ] {
        assert_eq!(
            owned_config(&authority.configured_owner(path)),
            Some("/ws/packages/legacy/jsconfig.json"),
            "{path} must bind through its authored source to jsconfig.json"
        );
    }
}

#[test]
fn a_file_no_configured_program_contains_is_terminal_no_project() {
    // Excluded from every configured project's membership by `node_modules/**`.
    // The deepest configured project that CONTAINS it on disk is `/ws`, and
    // answering `/ws` is exactly the invention the contract forbids: `/ws`'s
    // `include`/`files` do not cover this file, so its compiler options, its
    // aliases and its lib set are not the ones that apply to it. `NoProject` is
    // terminal — the consumer fails closed, it does not substitute an ancestor.
    let authority = monorepo_authority();
    assert_eq!(
        authority.configured_owner("/ws/node_modules/dep/index.d.ts"),
        ProjectOwnership::NoProject,
    );
}

#[test]
fn sibling_prefix_directory_is_not_treated_as_a_nested_package() {
    // `/ws/packages/app-extra` shares a string prefix with `/ws/packages/app`
    // but is NOT under it. A prefix-only containment test would bind it to the
    // wrong package's TypeScript.
    let resolver = ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
        "/ws/packages/app".to_string(),
        "/ws".to_string(),
        Some("/ws/packages/app/tsconfig.json".to_string()),
    )]);
    let snapshot = crate::test_utils::make_test_snapshot(
        resolver,
        &[(
            "/ws/packages/app",
            "/ws",
            Some("/ws/packages/app/tsconfig.json"),
        )],
    );
    let authority = SnapshotOwnerAuthority::new(snapshot);
    assert_eq!(
        authority.configured_owner("/ws/packages/app-extra/src/A.ts"),
        ProjectOwnership::NoProject,
    );
}

#[test]
fn a_fallback_only_workspace_yields_no_project_rather_than_an_invented_root() {
    let resolver = ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
        "/ws".to_string(),
        "/ws".to_string(),
        None,
    )]);
    // A fallback (inferred) project is NOT a configured owner.
    let snapshot = crate::test_utils::make_test_snapshot(resolver, &[("/ws", "/ws", None)]);
    let authority = SnapshotOwnerAuthority::new(snapshot);
    assert_eq!(
        authority.configured_owner("/ws/src/App.vue"),
        ProjectOwnership::NoProject
    );
}
