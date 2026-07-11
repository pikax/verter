//! Tests for the tsconfig-virtualization identity model.
//!
//! The virtual-config identity folds three inputs — the user config content,
//! the extends-ancestor contents, and the owned-companion-path set — into a
//! distinct project identity. Two invariants drive the design:
//!
//! 1. **No aliasing.** A virtualized config's identity MUST NOT equal the same
//!    config's NON-virtualized `IdeProjectConfig::project_identity()`. If they
//!    aliased, a virtualized Program and the real Program would share a cache
//!    slot — the divergence-is-the-bug class.
//! 2. **Precise invalidation.** The identity ADVANCES on a user-config content
//!    edit, a companion-path-set change, or an extends-ancestor edit; it stays
//!    STABLE across a pure carrier-TEXT edit (which changes neither the config
//!    bytes nor the companion path set).

use std::sync::Arc;

use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolver::{IdeProjectConfig, ProjectMembership};

use super::compute_virtual_config_identity;

const WORKSPACE_ROOT: &str = "d:/ws";
const TSCONFIG: &str = "d:/ws/tsconfig.json";

fn workspace_with(files: &[(&str, &str)]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![WORKSPACE_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for (path, content) in files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws
}

const VUE_TSCONFIG: &str = r#"{ "include": ["src/**/*.vue"] }"#;

fn companions() -> Vec<String> {
    vec!["d:/ws/src/Foo.vue.tsx".to_string()]
}

#[test]
fn virtual_identity_does_not_alias_non_virtual_project_identity() {
    let ws = workspace_with(&[(TSCONFIG, VUE_TSCONFIG)]);
    let virt = compute_virtual_config_identity(&ws, TSCONFIG, &companions());

    // The NON-virtual identity for the same configured project.
    let non_virtual = IdeProjectConfig {
        root: WORKSPACE_ROOT.to_string(),
        workspace_root: WORKSPACE_ROOT.to_string(),
        tsconfig_path: Some(TSCONFIG.to_string()),
        provider_root: WORKSPACE_ROOT.to_string(),
        workspace_aliases: Vec::new(),
        compiler_options: Default::default(),
        references: Vec::new(),
        membership: crate::snapshot_builder::configured_membership_from_raw(
            WORKSPACE_ROOT,
            &ProjectMembership::IncludeExclude {
                files: Vec::new(),
                include: vec!["d:/ws/src/**/*.vue".to_string()],
                exclude: Vec::new(),
            },
            &crate::resolver::IdeProjectCompilerOptions::default(),
        ),
    }
    .project_identity();

    assert_ne!(
        virt.to_hash16(),
        non_virtual,
        "a virtualized config MUST NOT alias the non-virtualized project_identity"
    );
}

#[test]
fn virtual_identity_advances_on_user_config_content_change() {
    let ws_a = workspace_with(&[(TSCONFIG, VUE_TSCONFIG)]);
    let a = compute_virtual_config_identity(&ws_a, TSCONFIG, &companions());

    // Same companions, same extends graph (none), but the user tsconfig bytes
    // changed (a different — still companion-non-enumerating — include).
    let ws_b = workspace_with(&[(TSCONFIG, r#"{ "include": ["app/**/*.vue"] }"#)]);
    let b = compute_virtual_config_identity(&ws_b, TSCONFIG, &companions());

    assert_ne!(
        a.to_hash16(),
        b.to_hash16(),
        "a user tsconfig content edit MUST advance the virtual identity"
    );
}

#[test]
fn virtual_identity_advances_on_companion_path_set_change() {
    let ws = workspace_with(&[(TSCONFIG, VUE_TSCONFIG)]);
    let one = compute_virtual_config_identity(&ws, TSCONFIG, &companions());

    // A carrier was added/renamed ⇒ the owned-companion-path set changed.
    let two_companions = vec![
        "d:/ws/src/Foo.vue.tsx".to_string(),
        "d:/ws/src/Bar.vue.tsx".to_string(),
    ];
    let two = compute_virtual_config_identity(&ws, TSCONFIG, &two_companions);

    assert_ne!(
        one.to_hash16(),
        two.to_hash16(),
        "a companion-path-set change (carrier added/removed/renamed) MUST advance the identity"
    );
}

#[test]
fn virtual_identity_is_companion_set_order_independent() {
    let ws = workspace_with(&[(TSCONFIG, VUE_TSCONFIG)]);
    let ascending = compute_virtual_config_identity(
        &ws,
        TSCONFIG,
        &[
            "d:/ws/src/Bar.vue.tsx".to_string(),
            "d:/ws/src/Foo.vue.tsx".to_string(),
        ],
    );
    let descending = compute_virtual_config_identity(
        &ws,
        TSCONFIG,
        &[
            "d:/ws/src/Foo.vue.tsx".to_string(),
            "d:/ws/src/Bar.vue.tsx".to_string(),
        ],
    );
    assert_eq!(
        ascending.to_hash16(),
        descending.to_hash16(),
        "the identity is over a SET — companion order must not change it"
    );
}

#[test]
fn virtual_identity_advances_on_extends_ancestor_change() {
    let child = r#"{ "extends": "./tsconfig.base.json", "include": ["src/**/*.vue"] }"#;
    let base_a = r#"{ "compilerOptions": { "strict": true } }"#;
    let base_b = r#"{ "compilerOptions": { "strict": false } }"#;

    let ws_a = workspace_with(&[(TSCONFIG, child), ("d:/ws/tsconfig.base.json", base_a)]);
    let a = compute_virtual_config_identity(&ws_a, TSCONFIG, &companions());

    // Only the EXTENDS ANCESTOR's content changed; the child tsconfig bytes and
    // the companion set are identical.
    let ws_b = workspace_with(&[(TSCONFIG, child), ("d:/ws/tsconfig.base.json", base_b)]);
    let b = compute_virtual_config_identity(&ws_b, TSCONFIG, &companions());

    assert_ne!(
        a.to_hash16(),
        b.to_hash16(),
        "an extends-ancestor content edit MUST advance the virtual identity"
    );
}

/// THE negative invariant. A pure carrier-TEXT edit changes neither the
/// tsconfig bytes, the extends graph, nor the companion PATH set — so the
/// virtual identity must be byte-identical. (Carrier content invalidation is a
/// content-hash concern handled elsewhere; the virtual CONFIG identity is over
/// the config + path SET only.)
#[test]
fn virtual_identity_is_stable_across_pure_carrier_text_edit() {
    // Edit 1: a carrier with some body.
    let ws_a = workspace_with(&[
        (TSCONFIG, VUE_TSCONFIG),
        ("d:/ws/src/Foo.vue", "<template>a</template>"),
    ]);
    let a = compute_virtual_config_identity(&ws_a, TSCONFIG, &companions());

    // Edit 2: the SAME carrier path, different body. Same tsconfig, same
    // companion path set.
    let ws_b = workspace_with(&[
        (TSCONFIG, VUE_TSCONFIG),
        (
            "d:/ws/src/Foo.vue",
            "<template>a totally different body</template>",
        ),
    ]);
    let b = compute_virtual_config_identity(&ws_b, TSCONFIG, &companions());

    assert_eq!(
        a.to_hash16(),
        b.to_hash16(),
        "a pure carrier-text edit MUST NOT advance the virtual config identity"
    );
}

/// Determinism: same inputs ⇒ same identity across calls.
#[test]
fn virtual_identity_is_deterministic() {
    let ws = workspace_with(&[(TSCONFIG, VUE_TSCONFIG)]);
    let a = compute_virtual_config_identity(&ws, TSCONFIG, &companions());
    let b = compute_virtual_config_identity(&ws, TSCONFIG, &companions());
    assert_eq!(a.to_hash16(), b.to_hash16(), "same inputs ⇒ same identity");
}
