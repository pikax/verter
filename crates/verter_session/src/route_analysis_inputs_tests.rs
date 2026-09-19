//! Tests for [`crate::route_analysis_inputs::build_route_analysis_inputs`]
//! — caller-side snapshot construction, verified end to end
//! against `verter_semantic::analysis::build_route_analysis` (the same
//! two-step pipeline production callers — `verter_lsp`'s `get_route_tree`,
//! `verter_mcp`'s `build_route_snapshot` — actually run).

use std::sync::Arc;

use crate::route_analysis_inputs::build_route_analysis_inputs;

fn fs_workspace() -> verter_workspace::FilesystemWorkspace {
    verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default())
}

fn canonical_str(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn captures_enough_for_a_vue_router_programmatic_config() {
    let tmp = verter_test_support::unique_temp_dir("route_inputs_vue_router");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src/router")).unwrap();

    std::fs::write(
        tmp.join("package.json"),
        r#"{ "dependencies": { "vue-router": "^4.2.0" } }"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/router/index.ts"),
        r#"
import { createRouter, createWebHistory } from 'vue-router'
const router = createRouter({
  history: createWebHistory(),
  routes: [{ path: '/', name: 'home', component: () => import('./Home.vue') }]
})
export default router
"#,
    )
    .unwrap();

    let root = canonical_str(&tmp);
    let inputs = build_route_analysis_inputs(&fs_workspace(), &root);

    let snapshot = verter_semantic::analysis::build_route_analysis(&inputs, &root, &[]);
    assert_eq!(
        snapshot.framework,
        verter_semantic::analysis::RoutingFramework::VueRouter
    );
    // Discriminates: if the walker had failed to capture
    // `src/router/index.ts`'s content, `discover_router_configs` would
    // still find the path exists (if it captured `insert_existing_file`
    // only), but `build_route_analysis` would extract zero routes from
    // an unreadable config — this proves the CONTENT, not just the
    // existence probe, made it into the snapshot.
    assert_eq!(snapshot.routes.len(), 1, "should extract the one route");
    assert_eq!(snapshot.routes[0].name.as_deref(), Some("home"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn captures_enough_for_nuxt_file_based_pages() {
    let tmp = verter_test_support::unique_temp_dir("route_inputs_nuxt_pages");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("pages")).unwrap();
    std::fs::create_dir_all(tmp.join("pages/users")).unwrap();

    std::fs::write(
        tmp.join("package.json"),
        r#"{ "dependencies": { "nuxt": "^3.8.0" } }"#,
    )
    .unwrap();
    std::fs::write(tmp.join("pages/index.vue"), "<template>Home</template>").unwrap();
    std::fs::write(
        tmp.join("pages/users/[id].vue"),
        "<template>User</template>",
    )
    .unwrap();

    let root = canonical_str(&tmp);
    let inputs = build_route_analysis_inputs(&fs_workspace(), &root);

    let snapshot = verter_semantic::analysis::build_route_analysis(&inputs, &root, &[]);
    assert_eq!(
        snapshot.framework,
        verter_semantic::analysis::RoutingFramework::NuxtPages
    );
    // Discriminates: if the walker's `pages/` recursion had stopped at
    // the top level (never descending into `pages/users/`), the dynamic
    // route below would be silently missing.
    assert_eq!(snapshot.routes.len(), 2, "index + dynamic nested route");
    assert!(snapshot.routes.iter().any(|r| r.full_path == "/"));
    assert!(snapshot.routes.iter().any(|r| r.full_path == "/users/:id"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn does_not_fabricate_router_config_candidates_that_do_not_exist() {
    let tmp = verter_test_support::unique_temp_dir("route_inputs_no_router_config");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("package.json"), "{}").unwrap();

    let root = canonical_str(&tmp);
    let inputs = build_route_analysis_inputs(&fs_workspace(), &root);

    for candidate in verter_semantic::analysis::ROUTER_CONFIG_CANDIDATES {
        let path = format!("{root}/{candidate}");
        assert!(
            inputs.read_file(&path).is_none(),
            "must not fabricate content for a candidate that was never on disk: {path}"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

fn memory_workspace() -> verter_workspace::MemoryWorkspace {
    verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default())
}

const ROUTER_PACKAGE: &str = r#"{ "dependencies": { "vue-router": "^4.2.0" } }"#;

/// Projection reads the committed rows only: a workspace write after the
/// commit must not leak into a projection of the ORIGINAL basis. The
/// end-to-end tests recapture after mutating, which cannot tell a
/// re-reading projection from a frozen one.
#[test]
fn projecting_the_original_basis_after_mutation_keeps_the_committed_content() {
    let ws = memory_workspace();
    let root = "/proj";
    ws.inject_file(format!("{root}/package.json"), Arc::from(ROUTER_PACKAGE));
    ws.inject_file(
        format!("{root}/src/router/index.ts"),
        Arc::from("export default { routes: [] }"),
    );
    let basis = crate::route_analysis_inputs::commit_route_analysis_basis(&ws, root);

    ws.inject_file(
        format!("{root}/package.json"),
        Arc::from(r#"{ "dependencies": {} }"#),
    );
    ws.inject_file(
        format!("{root}/src/router/index.ts"),
        Arc::from("export default { routes: [{ path: '/late' }] }"),
    );

    let projected = crate::route_analysis_inputs::project_route_analysis_inputs(&basis);
    assert_eq!(
        projected
            .read_file(&format!("{root}/package.json"))
            .as_deref(),
        Some(ROUTER_PACKAGE),
        "projection must not re-read the mutated workspace"
    );
    assert_eq!(
        projected
            .read_file(&format!("{root}/src/router/index.ts"))
            .as_deref(),
        Some("export default { routes: [] }")
    );
}

/// A regular file where a directory is expected is an observed file, not
/// an absence: the two capture outcomes must commit distinct bases.
#[test]
fn a_regular_file_at_a_directory_path_commits_a_file_observation_not_absence() {
    let root = "/proj";
    let absent = memory_workspace();
    absent.inject_file(format!("{root}/package.json"), Arc::from("{}"));
    let absent_basis = crate::route_analysis_inputs::commit_route_analysis_basis(&absent, root);
    assert!(
        matches!(
            absent_basis.observe(&format!("{root}/layouts")),
            Err(crate::input_basis::ObserveError::Negative(_))
        ),
        "a missing layouts path is a recorded absence"
    );

    let file = memory_workspace();
    file.inject_file(format!("{root}/package.json"), Arc::from("{}"));
    file.inject_file(format!("{root}/layouts"), Arc::from("not a directory"));
    let file_basis = crate::route_analysis_inputs::commit_route_analysis_basis(&file, root);
    assert_eq!(
        file_basis
            .observe(&format!("{root}/layouts"))
            .expect("an existing regular file is a positive observation")
            .file_content(),
        Some("not a directory")
    );
    assert_ne!(
        absent_basis.id(),
        file_basis.id(),
        "an existing file and an absent path must not share a basis identity"
    );

    let projected = crate::route_analysis_inputs::project_route_analysis_inputs(&file_basis);
    assert!(
        !projected.is_dir(&format!("{root}/layouts")),
        "the file is never projected as a directory"
    );
}

/// Two captures on one shared request can race to bind. The loser's basis
/// is discarded and the bound basis is projected; a workspace write
/// between the two captures must not turn the race into a panic. The
/// race is replayed deterministically: the request is bound first, then
/// the second capture's basis is handed to the bind-and-project step.
#[test]
fn a_rejected_bind_projects_the_bound_basis_instead_of_panicking() {
    let ws = memory_workspace();
    let root = "/proj";
    ws.inject_file(format!("{root}/package.json"), Arc::from(ROUTER_PACKAGE));
    let ctx = crate::request_context::RequestContext::new(
        7,
        Arc::from("/proj/package.json"),
        false,
        None,
    );
    let first = crate::route_analysis_inputs::commit_route_analysis_basis(&ws, root);
    let first_id = first.id().clone();
    ctx.bind_committed_input(first).expect("first bind");

    ws.inject_file(
        format!("{root}/package.json"),
        Arc::from(r#"{ "dependencies": {} }"#),
    );
    let second = crate::route_analysis_inputs::commit_route_analysis_basis(&ws, root);
    assert_ne!(
        second.id(),
        &first_id,
        "the mutated workspace commits a different basis"
    );
    let inputs = crate::route_analysis_inputs::bind_and_project_route_analysis_inputs(&ctx, second);
    assert_eq!(
        inputs.read_file(&format!("{root}/package.json")).as_deref(),
        Some(ROUTER_PACKAGE),
        "the bound basis wins over a later capture"
    );
    assert_eq!(
        ctx.committed_input().expect("still bound").basis().id(),
        &first_id
    );
}
