//! Tests for [`crate::route_analysis_inputs::build_route_analysis_inputs`]
//! — caller-side snapshot construction, verified end to end
//! against `verter_semantic::analysis::build_route_analysis` (the same
//! two-step pipeline production callers — `verter_lsp`'s `get_route_tree`,
//! `verter_mcp`'s `build_route_snapshot` — actually run).

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
