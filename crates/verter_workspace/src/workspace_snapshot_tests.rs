use super::*;
use crate::canonical_path::CanonicalPath;
use crate::membership::{ConfiguredMembership, FallbackMembership, StaticMembershipSpec};
use crate::normalized_glob::{CompiledGlob, NormalizedGlob};
use crate::resolver::{IdeProjectCompilerOptions, ProjectResolver};
use rustc_hash::FxHashSet;

// ── Helpers ──

/// A configured project with an explicit `include` glob (rooted at `root`).
///
/// Lets a test model a configured project whose ownership is genuinely
/// NARROWER than its root — e.g. two non-overlapping tsconfigs, or a file no
/// configured project owns. The materialized set is a walk-time POSITIVE cache
/// only, so ownership of a file absent from it is decided by this `include`
/// (via `ConfiguredMembership::contains`'s spec fall-through), NOT by the set.
fn configured_project_with_include(
    id: u32,
    root: &str,
    tsconfig: &str,
    include: &str,
    file_paths: &[&str],
) -> OwnershipProject {
    let root_cp = CanonicalPath::new(root);
    let spec = StaticMembershipSpec {
        files: Vec::new(),
        include: vec![CompiledGlob::new(NormalizedGlob::from_root_and_pattern(
            &root_cp, include,
        ))],
        exclude: vec![CompiledGlob::new(NormalizedGlob::from_root_and_pattern(
            &root_cp,
            "node_modules/**",
        ))]
        .into(),
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

/// A configured project claiming everything under `root` (match-all `**/*`
/// include), minus `node_modules`.
fn configured_project(
    id: u32,
    root: &str,
    tsconfig: &str,
    file_paths: &[&str],
) -> OwnershipProject {
    configured_project_with_include(id, root, tsconfig, "**/*", file_paths)
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
                exclude: vec![CompiledGlob::new(NormalizedGlob::new(&format!(
                    "{}/node_modules/**",
                    root
                )))]
                .into(),
            },
        },
    }
}

fn snapshot_with(mut projects: Vec<OwnershipProject>) -> WorkspaceSnapshot {
    projects.sort_by(compare_project_precedence);
    WorkspaceSnapshot {
        owners_memo: Default::default(),
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
    // The configured project's include is NARROWER than its root (only
    // `src/**/*`), so a file OUTSIDE that include is genuinely not a configured
    // member — regardless of the materialized set — and the fallback
    // (root-containment) is its only owner. (A match-all `**/*` configured
    // project would legitimately OWN any non-excluded file under its root, even
    // one absent from the walk-time materialized set, so it is the wrong
    // fixture for a "no configured owner" scenario.)
    let snap = snapshot_with(vec![
        configured_project_with_include(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            "src/**/*",
            &["d:/project/src/main.ts"],
        ),
        fallback_project(1, "d:/project"),
    ]);

    // `scripts/build.ts` is under the project root but OUTSIDE the configured
    // `src/**/*` include → no configured project owns it → fallback claims.
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
    // Genuinely non-overlapping: disjoint includes (`src/**/*` vs `tests/**/*`),
    // NOT two match-all `**/*` projects made to look disjoint by disjoint
    // materialized sets. Two `**/*` includes at the same root DO overlap, so a
    // spec-matching file absent from one project's walk-time set would still be
    // owned by it via the spec fall-through.
    let snap = snapshot_with(vec![
        configured_project_with_include(
            0,
            "d:/project",
            "d:/project/tsconfig.app.json",
            "src/**/*",
            &["d:/project/src/main.ts", "d:/project/src/app.vue"],
        ),
        configured_project_with_include(
            1,
            "d:/project",
            "d:/project/tsconfig.vitest.json",
            "tests/**/*",
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
    // Narrow configured include (`src/**/*`): `scripts/build.ts` is genuinely
    // not a configured member, so there is no configured resolution — even
    // though it is absent from the walk-time materialized set (which alone
    // never decides non-membership).
    let snap = snapshot_with(vec![
        configured_project_with_include(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            "src/**/*",
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
fn incomparable_configured_roots_overlap_is_ambiguous() {
    // FIX-6 (snapshot reachability): the genuine incomparable-overlap branch is
    // reachable ONLY in the snapshot path, because `ConfiguredMembership::contains`
    // matches `materialized_files` exact entries WITHOUT a root gate (unlike the
    // resolver path's `normalized_starts_with(file, root)`). Two configured
    // projects with INCOMPARABLE roots (neither an ancestor of the other) whose
    // materialized sets BOTH contain the exact same canonical file must resolve
    // to Ambiguous — the ancestor-pruning in `configured_owner_resolution_for_file`
    // prunes only STRICT ANCESTORS, so neither incomparable candidate is dropped.
    let snap = snapshot_with(vec![
        configured_project(
            0,
            "d:/project/packages/a",
            "d:/project/packages/a/tsconfig.json",
            &["d:/project/shared/util.ts"],
        ),
        configured_project(
            1,
            "d:/project/packages/b",
            "d:/project/packages/b/tsconfig.json",
            &["d:/project/shared/util.ts"],
        ),
    ]);

    // Discriminator: an implementation that picks a winner for incomparable
    // overlap (e.g. deepest/first root) would return Unique and fail here.
    let res = snap.configured_owner_resolution_for_file("d:/project/shared/util.ts");
    match res {
        ConfiguredOwnerResolution::Ambiguous(ids) => {
            assert_eq!(
                ids.len(),
                2,
                "both incomparable configs must remain, got {ids:?}"
            );
            assert!(ids.contains(&ProjectId(0)));
            assert!(ids.contains(&ProjectId(1)));
        }
        other => panic!("expected Ambiguous for incomparable-root overlap, got {other:?}"),
    }

    // The fallback lookup must refuse to invent a winner from a genuine configured
    // overlap: an incomparable configured overlap is NOT a single fallback owner.
    assert_eq!(
        snap.single_fallback_owner_for_file("d:/project/shared/util.ts"),
        None,
        "single_fallback_owner_for_file must be None for genuine incomparable configured overlap"
    );
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

// ── F2 deterministic graph pruning (aggregators / referenced non-leaves) ──

/// Set the `references` of a Configured project (by resolved tsconfig path).
fn with_references(mut project: OwnershipProject, refs: &[&str]) -> OwnershipProject {
    if let ProjectPayload::Configured { references, .. } = &mut project.payload {
        *references = refs.iter().map(|r| CanonicalPath::new(r)).collect();
    } else {
        panic!("with_references only applies to Configured projects");
    }
    project
}

#[test]
fn f2_solution_aggregator_referencing_leaf_resolves_to_unique_leaf() {
    // A solution/aggregator tsconfig at the SAME root that BOTH claims the file
    // (broad/default include materializes it) AND `references` the real leaf
    // config which also claims it. Pre-F2, both candidates survive at the same
    // root (neither is a strict ancestor) -> Ambiguous. F2: the aggregator
    // transitively references a co-claiming candidate, so it is dropped as a
    // non-leaf; the unique leaf wins. This is TS solution-style behaviour: the
    // referenced leaf owns the file, the aggregator only pulls it in.
    let leaf = configured_project(
        0,
        "d:/project",
        "d:/project/tsconfig.app.json",
        &["d:/project/src/App.vue"],
    );
    let aggregator = with_references(
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/App.vue"],
        ),
        &["d:/project/tsconfig.app.json"],
    );
    let snap = snapshot_with(vec![leaf, aggregator, fallback_project(2, "d:/project")]);

    // Discriminator: pre-F2 this is Ambiguous over both configs.
    match snap.configured_owner_resolution_for_file("d:/project/src/App.vue") {
        ConfiguredOwnerResolution::Unique(id) => {
            assert_eq!(
                snap.tsconfig_path(id).map(|p| p.as_str()),
                Some("d:/project/tsconfig.app.json"),
                "the referenced leaf must be the unique owner, not the aggregator"
            );
        }
        other => panic!("expected Unique(leaf), got {other:?}"),
    }
}

#[test]
fn f2_reference_chain_root_mid_leaf_resolves_to_unique_leaf() {
    // Transitive: root -> mid -> leaf, all three claim the same file at the same
    // root. Pre-F2 -> Ambiguous (3 candidates, none a strict ancestor). F2: root
    // transitively references leaf (via mid) and mid references leaf, so both are
    // dropped as non-leaves; only the terminal leaf survives.
    let root = with_references(
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.json",
            &["d:/project/src/App.vue"],
        ),
        &["d:/project/tsconfig.mid.json"],
    );
    let leaf = configured_project(
        1,
        "d:/project",
        "d:/project/tsconfig.leaf.json",
        &["d:/project/src/App.vue"],
    );
    let mid = with_references(
        configured_project(
            2,
            "d:/project",
            "d:/project/tsconfig.mid.json",
            &["d:/project/src/App.vue"],
        ),
        &["d:/project/tsconfig.leaf.json"],
    );
    let snap = snapshot_with(vec![root, leaf, mid, fallback_project(3, "d:/project")]);

    match snap.configured_owner_resolution_for_file("d:/project/src/App.vue") {
        ConfiguredOwnerResolution::Unique(id) => {
            assert_eq!(
                snap.tsconfig_path(id).map(|p| p.as_str()),
                Some("d:/project/tsconfig.leaf.json"),
                "the terminal leaf must be the unique owner across the reference chain"
            );
        }
        other => panic!("expected Unique(leaf) across the chain, got {other:?}"),
    }
}

#[test]
fn f2_same_root_overlap_without_reference_edge_stays_ambiguous() {
    // Control: two real configs at the same root that BOTH claim the file with NO
    // reference edge between them is a GENUINE tie -> Ambiguous. F2 pruning must
    // NOT invent a winner here (no aggregator/leaf relationship exists).
    let app = configured_project(
        0,
        "d:/project",
        "d:/project/tsconfig.app.json",
        &["d:/project/src/shared.ts"],
    );
    let test = configured_project(
        1,
        "d:/project",
        "d:/project/tsconfig.test.json",
        &["d:/project/src/shared.ts"],
    );
    let snap = snapshot_with(vec![app, test, fallback_project(2, "d:/project")]);

    match snap.configured_owner_resolution_for_file("d:/project/src/shared.ts") {
        ConfiguredOwnerResolution::Ambiguous(ids) => {
            assert_eq!(ids.len(), 2, "genuine same-root tie keeps both candidates");
        }
        other => panic!("expected Ambiguous for a genuine tie, got {other:?}"),
    }
}

#[test]
fn f2_cyclic_reference_pair_stays_ambiguous_not_none() {
    // A MALFORMED reference cycle: two configs at the same root that BOTH claim the
    // file AND mutually `reference` each other (A -> B and B -> A). They form ONE
    // reference SCC in which each transitively reaches the other, so neither is a
    // strict solution aggregator over the other. Without SCC-condensing, the (b)
    // domination pass dropped A (it references B) AND dropped B (it references A) ->
    // effective set empty -> None -> a FALSE NoProject. Condensing the SCC (a strict
    // reference edge requires the reverse edge to be absent) keeps BOTH candidates:
    // a genuine cyclic tie is Ambiguous, never silently None.
    let a = with_references(
        configured_project(
            0,
            "d:/project",
            "d:/project/tsconfig.a.json",
            &["d:/project/src/App.vue"],
        ),
        &["d:/project/tsconfig.b.json"],
    );
    let b = with_references(
        configured_project(
            1,
            "d:/project",
            "d:/project/tsconfig.b.json",
            &["d:/project/src/App.vue"],
        ),
        &["d:/project/tsconfig.a.json"],
    );
    let snap = snapshot_with(vec![a, b, fallback_project(2, "d:/project")]);

    match snap.configured_owner_resolution_for_file("d:/project/src/App.vue") {
        ConfiguredOwnerResolution::Ambiguous(ids) => {
            assert_eq!(
                ids.len(),
                2,
                "a cyclic A<->B reference tie must keep BOTH candidates, never collapse to None"
            );
        }
        other => panic!("expected Ambiguous over the cyclic reference pair, got {other:?}"),
    }
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
    let mut projects = [
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

// ── owners_for_file glob-membership pins ──
//
// Pins the exact ownership results for the three membership classes the
// glob match loops decide: an exact file inside the root, a path matching
// the default TypeScript excludes the memberships actually hold
// ({root}/node_modules/**), and a non-member path outside every root.
// Bridge-mode configured membership (empty materialized set) is used so the
// include/exclude glob loops — not the materialized FxHashSet — decide.

#[test]
fn owners_for_file_pins_glob_membership_classes() {
    let root_cp = CanonicalPath::new("d:/project");
    let bridge_configured = OwnershipProject {
        id: ProjectId(0),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new("d:/project/tsconfig.json"),
            membership: ConfiguredMembership {
                spec: StaticMembershipSpec::with_typescript_defaults(&root_cp),
                materialized_files: FxHashSet::default(), // bridge → glob loops decide
            },
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            workspace_aliases: Vec::new(),
        },
    };
    let snap = snapshot_with(vec![bridge_configured, fallback_project(1, "d:/project")]);

    // Exact file inside the root: configured claims via include globs; the
    // fallback stays out because a configured owner already claimed it.
    let owners = snap.owners_for_file("d:/project/src/main.ts");
    assert_eq!(owners.len(), 1, "exactly the configured project must own");
    assert_eq!(owners[0], ProjectId(0));

    // File under node_modules: both memberships hold {root}/node_modules/**
    // exclude patterns, so nobody claims it.
    assert!(
        snap.owners_for_file("d:/project/node_modules/vue/index.ts")
            .is_empty(),
        "node_modules paths must stay unowned"
    );

    // Non-member path outside every root: no owners.
    assert!(
        snap.owners_for_file("d:/elsewhere/foo.ts").is_empty(),
        "paths outside every root must stay unowned"
    );
}

// ── Edge case: empty snapshot ──

#[test]
fn empty_snapshot_has_no_owners() {
    let snap = WorkspaceSnapshot {
        owners_memo: Default::default(),
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

// ── owners_for_file memo (snapshot-owned, bounded, negative-caching) ──

#[test]
fn memoized_owners_equal_fresh_compute_for_member_excluded_and_non_member() {
    // Spec-only configured project (empty materialized set) so the glob
    // matching path — the expensive path the memo exists for — is exercised.
    let snap = snapshot_with(vec![
        configured_project(0, "d:/proj", "d:/proj/tsconfig.json", &[]),
        fallback_project(1, "d:/proj"),
    ]);

    let member = "d:/proj/src/app.vue";
    let excluded = "d:/proj/node_modules/dep/index.ts";
    let non_member = "d:/elsewhere/foo.ts";

    for path in [member, excluded, non_member] {
        let cold = snap.owners_for_file(path);
        assert_eq!(
            cold,
            snap.compute_owners_for_file(path),
            "cold memoized answer must equal a fresh compute for {path}"
        );
        let warm = snap.owners_for_file(path);
        assert_eq!(
            warm, cold,
            "warm (memo-hit) answer must equal cold for {path}"
        );
    }

    // Sanity on the actual answers…
    assert_eq!(snap.owners_for_file(member).as_slice(), &[ProjectId(0)]);
    assert!(snap.owners_for_file(excluded).is_empty());
    assert!(snap.owners_for_file(non_member).is_empty());

    // …and every result — including the two negative (empty) ones — is memoized.
    assert_eq!(
        snap.owners_memo.map.len(),
        3,
        "negative (empty owner set) results are cached too"
    );

    // Downstream resolution built on memoized reads stays stable across calls.
    assert_eq!(
        snap.configured_owner_resolution_for_file(member),
        ConfiguredOwnerResolution::Unique(ProjectId(0))
    );
    assert_eq!(
        snap.configured_owner_resolution_for_file(member),
        ConfiguredOwnerResolution::Unique(ProjectId(0))
    );
}

#[test]
fn memo_hit_is_served_from_the_memo_not_recomputed() {
    let snap = snapshot_with(vec![
        configured_project(0, "d:/proj", "d:/proj/tsconfig.json", &[]),
        fallback_project(1, "d:/proj"),
    ]);

    let member = "d:/proj/src/app.vue";
    assert_eq!(snap.owners_for_file(member).as_slice(), &[ProjectId(0)]);

    // Overwrite the memoized entry with a sentinel: a hit must come back from
    // the memo verbatim. This discriminates "memo is actually read on hit"
    // from "memo is written but every call still recomputes".
    let sentinel: SmallVec<[ProjectId; 2]> = SmallVec::from_slice(&[ProjectId(9)]);
    snap.owners_memo.insert(member, sentinel.clone());
    assert_eq!(snap.owners_for_file(member), sentinel);
}

#[test]
fn new_snapshot_starts_with_a_fresh_memo() {
    let path = "d:/proj/src/main.ts";

    let snap_a = snapshot_with(vec![
        configured_project(0, "d:/proj", "d:/proj/tsconfig.json", &[path]),
        fallback_project(1, "d:/proj"),
    ]);
    assert_eq!(snap_a.owners_for_file(path).as_slice(), &[ProjectId(0)]);

    // Same path, different snapshot, different membership: the memo is
    // snapshot-owned, so snapshot B answers from ITS OWN state, never A's.
    // B's configured project has a NARROWER include (`sub/**/*`) that does not
    // cover `path`, so `path` is genuinely unowned by configured in B and the
    // fallback claims it — a different answer than A, which is the point.
    let snap_b = snapshot_with(vec![
        configured_project_with_include(
            0,
            "d:/proj",
            "d:/proj/tsconfig.json",
            "sub/**/*",
            &["d:/proj/sub/other.ts"],
        ),
        fallback_project(1, "d:/proj"),
    ]);
    assert_eq!(
        snap_b.owners_memo.map.len(),
        0,
        "a freshly built snapshot starts with an empty memo"
    );
    assert_eq!(
        snap_b.owners_for_file(path).as_slice(),
        &[ProjectId(1)],
        "snapshot B answers from its own membership (fallback), not A's memo"
    );
}

#[test]
fn memo_bound_clears_on_overflow_and_keeps_answers_correct() {
    // Production default is bounded at OWNERS_MEMO_CAP…
    assert_eq!(OwnersMemo::default().cap, OWNERS_MEMO_CAP);

    // …tests exercise the same clear-on-overflow logic with a tiny cap.
    let mut snap = snapshot_with(vec![
        configured_project(0, "d:/proj", "d:/proj/tsconfig.json", &[]),
        fallback_project(1, "d:/proj"),
    ]);
    snap.owners_memo = OwnersMemo::with_cap(4);

    for i in 0..4 {
        let _ = snap.owners_for_file(&format!("d:/elsewhere/f{i}.ts"));
    }
    assert_eq!(
        snap.owners_memo.map.len(),
        4,
        "distinct queries fill the memo up to its cap"
    );

    // One more distinct key: overflow clears the memo, then admits the entry.
    let over = snap.owners_for_file("d:/proj/src/over.ts");
    assert_eq!(over.as_slice(), &[ProjectId(0)]);
    assert_eq!(
        snap.owners_memo.map.len(),
        1,
        "overflowing insert clears the memo first, then admits the new entry"
    );

    // Evicted keys recompute correctly on the next query.
    assert!(snap.owners_for_file("d:/elsewhere/f0.ts").is_empty());
    assert_eq!(snap.owners_memo.map.len(), 2);
}
