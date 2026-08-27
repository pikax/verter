use super::{
    effective_configs_for_path, nearest_config_for_path, project_for_ownership,
    sorted_by_precedence,
};

fn configured_project(root: &str, tsconfig: &str) -> crate::resolver_core::IdeProjectConfig {
    crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    )
}

fn fallback_project(root: &str) -> crate::resolver_core::IdeProjectConfig {
    crate::resolver_core::IdeProjectConfig::new(root.to_string(), root.to_string(), None)
}

#[test]
fn sorted_by_precedence_orders_the_deeper_root_first() {
    let projects = sorted_by_precedence(vec![
        configured_project("/proj", "/proj/tsconfig.json"),
        configured_project("/proj/pkg", "/proj/pkg/tsconfig.json"),
    ]);

    assert_eq!(projects[0].root, "/proj/pkg");
    assert_eq!(projects[1].root, "/proj");
}

#[test]
fn effective_configs_for_path_finds_the_claiming_configured_project() {
    let projects = sorted_by_precedence(vec![configured_project("/proj", "/proj/tsconfig.json")]);

    let configs = effective_configs_for_path(&projects, "/proj/src/main.ts");
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].root, "/proj");
}

#[test]
fn effective_configs_for_path_prunes_a_strict_ancestor_root() {
    let projects = sorted_by_precedence(vec![
        configured_project("/proj", "/proj/tsconfig.json"),
        configured_project("/proj/pkg", "/proj/pkg/tsconfig.json"),
    ]);

    // "/proj/pkg/src/main.ts" is claimed by BOTH configs (both are
    // match-all-under-root) — the deeper "/proj/pkg" must survive and
    // the strict-ancestor "/proj" must be pruned, not both retained.
    let configs = effective_configs_for_path(&projects, "/proj/pkg/src/main.ts");
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].root, "/proj/pkg");
}

#[test]
fn effective_configs_for_path_falls_back_when_no_configured_project_matches() {
    let projects = sorted_by_precedence(vec![fallback_project("/proj")]);

    let configs = effective_configs_for_path(&projects, "/proj/src/main.ts");
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].root, "/proj");
    assert!(configs[0].tsconfig_path.is_none());
}

#[test]
fn effective_configs_for_path_finds_nothing_outside_every_root() {
    let projects = sorted_by_precedence(vec![configured_project("/proj", "/proj/tsconfig.json")]);

    let configs = effective_configs_for_path(&projects, "/elsewhere/main.ts");
    assert!(configs.is_empty());
}

#[test]
fn nearest_config_for_path_returns_the_first_effective_config() {
    let projects = sorted_by_precedence(vec![
        configured_project("/proj", "/proj/tsconfig.json"),
        configured_project("/proj/pkg", "/proj/pkg/tsconfig.json"),
    ]);

    let nearest = nearest_config_for_path(&projects, "/proj/pkg/src/main.ts");
    assert_eq!(nearest.map(|p| p.root.as_str()), Some("/proj/pkg"));
}

#[test]
fn project_for_ownership_finds_an_exact_root_and_tsconfig_match() {
    let projects = vec![
        configured_project("/proj/a", "/proj/a/tsconfig.json"),
        configured_project("/proj/b", "/proj/b/tsconfig.json"),
    ];

    let owner = crate::resolver_core::ProjectOwnership {
        project_root: "/proj/a".to_string(),
        tsconfig_path: Some("/proj/a/tsconfig.json".to_string()),
    };
    let found = project_for_ownership(&projects, &owner);
    assert_eq!(found.map(|p| p.root.as_str()), Some("/proj/a"));
}

#[test]
fn project_for_ownership_refuses_a_genuine_duplicate() {
    let projects = vec![
        configured_project("/proj/a", "/proj/a/tsconfig.json"),
        configured_project("/proj/a", "/proj/a/tsconfig.json"),
    ];

    let owner = crate::resolver_core::ProjectOwnership {
        project_root: "/proj/a".to_string(),
        tsconfig_path: Some("/proj/a/tsconfig.json".to_string()),
    };
    assert!(project_for_ownership(&projects, &owner).is_none());
}

#[test]
fn project_for_ownership_misses_an_unregistered_root() {
    let projects = vec![configured_project("/proj/a", "/proj/a/tsconfig.json")];

    let owner = crate::resolver_core::ProjectOwnership {
        project_root: "/proj/missing".to_string(),
        tsconfig_path: None,
    };
    assert!(project_for_ownership(&projects, &owner).is_none());
}
