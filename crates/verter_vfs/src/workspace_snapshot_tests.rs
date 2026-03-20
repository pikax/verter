use super::*;
use crate::canonical_path::CanonicalPath;
use crate::membership::{ConfiguredMembership, FallbackMembership, StaticMembershipSpec};
use crate::normalized_glob::NormalizedGlob;
use crate::resolver::{IdeProjectCompilerOptions, ProjectResolver};
use rustc_hash::FxHashSet;

// ── Helpers ──

fn configured_project(
    id: u32,
    root: &str,
    tsconfig: &str,
    file_paths: &[&str],
) -> OwnershipProject {
    let root_cp = CanonicalPath::new(root);
    let spec = StaticMembershipSpec {
        files: Vec::new(),
        include: vec![NormalizedGlob::from_root_and_pattern(&root_cp, "**/*")],
        exclude: vec![NormalizedGlob::from_root_and_pattern(
            &root_cp,
            "node_modules/**",
        )],
    };
    let mut materialized = FxHashSet::default();
    for path in file_paths {
        materialized.insert(CanonicalPath::new(path));
    }

    OwnershipProject {
        id: ProjectId(id),
        root: root_cp.clone(),
        workspace_root: root_cp,
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: materialized,
            },
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            workspace_aliases: Vec::new(),
        },
    }
}

fn fallback_project(id: u32, root: &str) -> OwnershipProject {
    let root_cp = CanonicalPath::new(root);
    OwnershipProject {
        id: ProjectId(id),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload: ProjectPayload::Fallback {
            membership: FallbackMembership {
                root: root_cp,
                exclude: vec![NormalizedGlob::new(&format!("{}/node_modules/**", root))],
            },
        },
    }
}

fn snapshot_with(mut projects: Vec<OwnershipProject>) -> WorkspaceSnapshot {
    projects.sort_by(compare_project_precedence);
    WorkspaceSnapshot {
        projects,
        resolver: ProjectResolver::default(),
        generation: SnapshotGeneration(1),
    }
}

// ── owners_for_file basic ──

#[test]
fn configured_owner_for_materialized_file() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts", "d:/project/src/app.vue"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0], ProjectId(0));
}

#[test]
fn fallback_claims_when_no_configured_owner() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    // File NOT in configured project's materialized set
    let owners = snap.owners_for_file("d:/project/scripts/build.ts");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0], ProjectId(1), "fallback should claim");
}

#[test]
fn no_owner_outside_all_roots() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    let owners = snap.owners_for_file("d:/other/foo.ts");
    assert!(
        owners.is_empty(),
        "file outside all roots should have no owner"
    );
}

#[test]
fn fallback_does_not_claim_when_configured_owns() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(owners.len(), 1, "only configured should own");
    assert_eq!(owners[0], ProjectId(0));
    // Negative: fallback should NOT be in the list
    assert!(
        !owners.contains(&ProjectId(1)),
        "fallback must not claim when configured owns"
    );
}

// ── Multi-owner / overlap ──

#[test]
fn non_overlapping_tsconfigs_unique_owner() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.app.json",
            &["d:/project/src/main.ts", "d:/project/src/app.vue"],
        ),
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.vitest.json",
            &["d:/project/tests/foo.spec.ts"],
        ),
        fallback_project(2, "d:/project"),
    ]);

    // src file → only tsconfig.app
    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0], ProjectId(0));

    // test file → only tsconfig.vitest
    let owners = snap.owners_for_file("d:/project/tests/foo.spec.ts");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0], ProjectId(1));
}

#[test]
fn overlapping_tsconfigs_both_returned() {
    // Both tsconfigs claim the same file
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.app.json",
            &["d:/project/src/shared.ts"],
        ),
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.vitest.json",
            &["d:/project/src/shared.ts"],
        ),
        fallback_project(2, "d:/project"),
    ]);

    let owners = snap.owners_for_file("d:/project/src/shared.ts");
    assert_eq!(owners.len(), 2, "both configured projects should claim");
    // Fallback should NOT be present
    assert!(!owners.contains(&ProjectId(2)));
}

// ── configured_owner_resolution_for_file ──

#[test]
fn unique_configured_resolution() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    let res = snap.configured_owner_resolution_for_file("d:/project/src/main.ts");
    assert_eq!(res, ConfiguredOwnerResolution::Unique(ProjectId(0)));
}

#[test]
fn no_configured_resolution_for_fallback_only() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    let res = snap.configured_owner_resolution_for_file("d:/project/scripts/build.ts");
    assert_eq!(res, ConfiguredOwnerResolution::None);
}

#[test]
fn ambiguous_configured_resolution_for_overlap() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.app.json",
            &["d:/project/src/shared.ts"],
        ),
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.vitest.json",
            &["d:/project/src/shared.ts"],
        ),
        fallback_project(2, "d:/project"),
    ]);

    let res = snap.configured_owner_resolution_for_file("d:/project/src/shared.ts");
    match res {
        ConfiguredOwnerResolution::Ambiguous(ids) => {
            assert_eq!(ids.len(), 2);
            assert!(ids.contains(&ProjectId(0)));
            assert!(ids.contains(&ProjectId(1)));
        }
        other => panic!("expected Ambiguous, got {:?}", other),
    }
}

#[test]
fn no_configured_resolution_outside_all_projects() {
    let snap = snapshot_with(vec![
        configured_project(0, "d:/project", "d:/project/tsconfig.json", &[]),
        fallback_project(1, "d:/project"),
    ]);

    let res = snap.configured_owner_resolution_for_file("d:/other/foo.ts");
    assert_eq!(res, ConfiguredOwnerResolution::None);
}

// ── Precedence ordering ──

#[test]
fn longer_root_takes_precedence() {
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/packages/ui/src/button.vue"],
        ),
        configured_project(
            1,
            "d:/project/packages/ui",
            "d:/project/packages/ui/tsconfig.json",
            &["d:/project/packages/ui/src/button.vue"],
        ),
    ]);

    // More specific root should be first in owners list
    let owners = snap.owners_for_file("d:/project/packages/ui/src/button.vue");
    assert_eq!(owners[0], ProjectId(1), "longer root should be first");
}

#[test]
fn configured_before_fallback_at_same_root() {
    let snap = snapshot_with(vec![
        fallback_project(1, "d:/project"),
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/main.ts"],
        ),
    ]);

    // Configured should be first despite being added second
    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(
        owners[0],
        ProjectId(0),
        "configured should be before fallback"
    );
}

#[test]
fn alphabetical_tiebreak_for_same_root_configured() {
    let snap = snapshot_with(vec![
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.vitest.json",
            &["d:/project/src/foo.ts"],
        ),
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.app.json",
            &["d:/project/src/foo.ts"],
        ),
    ]);

    // tsconfig.app.json < tsconfig.vitest.json alphabetically
    let owners = snap.owners_for_file("d:/project/src/foo.ts");
    assert_eq!(
        owners[0],
        ProjectId(0),
        "alphabetically earlier tsconfig should be first"
    );
}

// ── Fallback isolation ──

#[test]
fn fallback_excludes_node_modules() {
    let snap = snapshot_with(vec![fallback_project(0, "d:/project")]);

    let owners = snap.owners_for_file("d:/project/node_modules/vue/index.ts");
    assert!(owners.is_empty(), "fallback should exclude node_modules");
}

#[test]
fn fallback_rejects_partial_prefix() {
    let snap = snapshot_with(vec![fallback_project(0, "d:/project")]);

    let owners = snap.owners_for_file("d:/project-extra/foo.ts");
    assert!(
        owners.is_empty(),
        "project-extra should NOT match project root"
    );
}

// ── Accessor helpers ──

#[test]
fn project_accessor() {
    let snap = snapshot_with(vec![configured_project(
        0,
        "d:/project",
        "d:/project/tsconfig.json",
        &[],
    )]);

    let p = snap.project(ProjectId(0));
    assert!(p.root.as_str().contains("project"));
}

#[test]
fn is_configured_accessor() {
    let snap = snapshot_with(vec![
        configured_project(0, "d:/project", "d:/project/tsconfig.json", &[]),
        fallback_project(1, "d:/project"),
    ]);

    assert!(snap.is_configured(ProjectId(0)));
    assert!(!snap.is_configured(ProjectId(1)));
}

#[test]
fn tsconfig_path_accessor() {
    let snap = snapshot_with(vec![
        configured_project(0, "d:/project", "d:/project/tsconfig.json", &[]),
        fallback_project(1, "d:/project"),
    ]);

    assert!(snap.tsconfig_path(ProjectId(0)).is_some());
    assert!(snap.tsconfig_path(ProjectId(1)).is_none());
}

// ── SnapshotGeneration ──

#[test]
fn generation_increments() {
    let gen = SnapshotGeneration(5);
    assert_eq!(gen.next(), SnapshotGeneration(6));
}

#[test]
fn generation_default_is_zero() {
    assert_eq!(SnapshotGeneration::default(), SnapshotGeneration(0));
}

// ── compare_project_precedence ──

#[test]
fn precedence_sort_is_stable() {
    let mut projects = vec![
        fallback_project(2, "d:/project"),
        configured_project(1, "d:/project", "d:/project/tsconfig.vitest.json", &[]),
        configured_project(0, "d:/project", "d:/project/tsconfig.app.json", &[]),
        configured_project(
            3,
            "d:/project/packages/ui",
            "d:/project/packages/ui/tsconfig.json",
            &[],
        ),
    ];

    projects.sort_by(compare_project_precedence);

    // Longest root first
    assert_eq!(projects[0].id, ProjectId(3), "packages/ui (longest root)");
    // Then same-root: configured (alphabetical) before fallback
    assert_eq!(projects[1].id, ProjectId(0), "tsconfig.app.json");
    assert_eq!(projects[2].id, ProjectId(1), "tsconfig.vitest.json");
    assert_eq!(projects[3].id, ProjectId(2), "fallback");
}

// ── Fallback settings isolation (type system) ──

#[test]
fn fallback_project_has_no_configured_fields() {
    let project = fallback_project(0, "d:/project");
    match &project.payload {
        ProjectPayload::Fallback { membership } => {
            // Type system ensures no tsconfig_path, compiler_options, etc.
            assert!(membership.contains(&CanonicalPath::new("d:/project/foo.ts")));
        }
        ProjectPayload::Configured { .. } => {
            panic!("fallback should not be configured");
        }
    }
}

// ── Edge case: empty snapshot ──

#[test]
fn empty_snapshot_has_no_owners() {
    let snap = WorkspaceSnapshot {
        projects: vec![],
        resolver: ProjectResolver::default(),
        generation: SnapshotGeneration(0),
    };

    assert!(snap.owners_for_file("d:/anything.ts").is_empty());
    assert_eq!(
        snap.configured_owner_resolution_for_file("d:/anything.ts"),
        ConfiguredOwnerResolution::None
    );
}
