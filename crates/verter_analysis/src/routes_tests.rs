use super::*;

// =============================================================================
// Framework Detection Tests
// =============================================================================

#[test]
fn test_detect_vue_router() {
    let json = r#"{ "dependencies": { "vue": "^3.3.0", "vue-router": "^4.2.0" } }"#;
    assert_eq!(
        detect_routing_framework_from_json(json),
        RoutingFramework::VueRouter
    );
}

#[test]
fn test_detect_nuxt() {
    let json = r#"{ "dependencies": { "nuxt": "^3.8.0" } }"#;
    assert_eq!(
        detect_routing_framework_from_json(json),
        RoutingFramework::NuxtPages
    );
}

#[test]
fn test_detect_nuxt_with_vue_router() {
    // Nuxt includes vue-router internally — nuxt should win
    let json = r#"{ "dependencies": { "nuxt": "^3.8.0", "vue-router": "^4.2.0" } }"#;
    assert_eq!(
        detect_routing_framework_from_json(json),
        RoutingFramework::NuxtPages
    );
}

#[test]
fn test_detect_unplugin_vue_router() {
    let json = r#"{ "devDependencies": { "unplugin-vue-router": "^0.7.0" }, "dependencies": { "vue-router": "^4.2.0" } }"#;
    assert_eq!(
        detect_routing_framework_from_json(json),
        RoutingFramework::UnpluginVueRouter
    );
}

#[test]
fn test_detect_unknown() {
    let json = r#"{ "dependencies": { "vue": "^3.3.0" } }"#;
    assert_eq!(
        detect_routing_framework_from_json(json),
        RoutingFramework::Unknown
    );
}

#[test]
fn test_detect_invalid_json() {
    assert_eq!(
        detect_routing_framework_from_json("not json"),
        RoutingFramework::Unknown
    );
}

#[test]
fn test_detect_empty_json() {
    assert_eq!(
        detect_routing_framework_from_json("{}"),
        RoutingFramework::Unknown
    );
}

// =============================================================================
// Programmatic Route Extraction Tests
// =============================================================================

#[test]
fn test_extract_create_router_routes() {
    let content = r#"
import { createRouter, createWebHistory } from 'vue-router'
import HomeView from './views/HomeView.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'home',
      component: HomeView
    },
    {
      path: '/about',
      name: 'about',
      component: () => import('./views/AboutView.vue')
    }
  ]
})

export default router
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 2, "should extract 2 routes");

    // Home route
    assert_eq!(routes[0].path, "/");
    assert_eq!(routes[0].name.as_deref(), Some("home"));
    assert_eq!(
        routes[0].component_path.as_deref(),
        Some("./views/HomeView.vue")
    );
    assert!(!routes[0].is_lazy, "eager import should not be lazy");

    // About route
    assert_eq!(routes[1].path, "/about");
    assert_eq!(routes[1].name.as_deref(), Some("about"));
    assert_eq!(
        routes[1].component_path.as_deref(),
        Some("./views/AboutView.vue")
    );
    assert!(routes[1].is_lazy, "dynamic import should be lazy");

    // Full paths
    assert_eq!(routes[0].full_path, "/");
    assert_eq!(routes[1].full_path, "/about");
}

#[test]
fn test_extract_nested_routes() {
    let content = r#"
import { createRouter } from 'vue-router'

const router = createRouter({
  routes: [
    {
      path: '/admin',
      name: 'admin',
      component: () => import('./views/Admin.vue'),
      children: [
        {
          path: 'users',
          name: 'admin-users',
          component: () => import('./views/AdminUsers.vue')
        },
        {
          path: 'settings',
          name: 'admin-settings',
          component: () => import('./views/AdminSettings.vue')
        }
      ]
    }
  ]
})
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 1, "should have 1 top-level route");
    assert_eq!(routes[0].path, "/admin");
    assert_eq!(routes[0].children.len(), 2, "should have 2 children");

    assert_eq!(routes[0].children[0].path, "users");
    assert_eq!(routes[0].children[0].full_path, "/admin/users");
    assert_eq!(routes[0].children[0].name.as_deref(), Some("admin-users"));

    assert_eq!(routes[0].children[1].path, "settings");
    assert_eq!(routes[0].children[1].full_path, "/admin/settings");
}

#[test]
fn test_extract_redirect_route() {
    let content = r#"
const routes = [
  { path: '/old', redirect: '/new' },
  { path: '/new', name: 'new-page', component: () => import('./New.vue') }
]
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].redirect.as_deref(), Some("/new"));
    assert!(routes[1].redirect.is_none());
}

#[test]
fn test_extract_route_with_meta() {
    let content = r#"
const routes = [
  {
    path: '/dashboard',
    name: 'dashboard',
    component: () => import('./Dashboard.vue'),
    meta: { requiresAuth: 'true', title: 'Dashboard' }
  }
]
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].meta.len(), 2);
    assert!(
        routes[0].meta.iter().any(|(k, _)| k == "requiresAuth"),
        "should have requiresAuth meta"
    );
    assert!(
        routes[0]
            .meta
            .iter()
            .any(|(k, v)| k == "title" && v == "Dashboard"),
        "should have title meta"
    );
}

#[test]
fn test_extract_route_with_before_enter() {
    let content = r#"
const routes = [
  {
    path: '/protected',
    component: () => import('./Protected.vue'),
    beforeEnter: (to, from) => { return false }
  }
]
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].guards.len(), 1);
    assert_eq!(routes[0].guards[0].kind, RouteGuardKind::BeforeEnter);
}

#[test]
fn test_extract_export_const_routes() {
    let content = r#"
export const routes = [
  { path: '/', component: () => import('./Home.vue') },
  { path: '/about', component: () => import('./About.vue') }
]
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].path, "/");
    assert_eq!(routes[1].path, "/about");
}

#[test]
fn test_extract_empty_routes_array() {
    let content = r#"
import { createRouter } from 'vue-router'
const router = createRouter({ routes: [] })
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert!(
        routes.is_empty(),
        "should return empty vec for empty routes array"
    );
}

#[test]
fn test_no_routes_found() {
    let content = r#"
// No router configuration here
export function setup() { return {} }
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.ts", project_root);

    assert!(
        routes.is_empty(),
        "should return empty vec when no routes found"
    );
}

// =============================================================================
// File-Based Route Extraction Tests
// =============================================================================

#[test]
fn test_file_based_routes_with_temp_dir() {
    let tmp = std::env::temp_dir().join("verter_test_file_routes");
    let pages = tmp.join("pages");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&pages).unwrap();

    // Create test files
    std::fs::write(pages.join("index.vue"), "<template>Home</template>").unwrap();
    std::fs::write(pages.join("about.vue"), "<template>About</template>").unwrap();

    let routes = extract_file_based_routes(&pages);

    assert_eq!(routes.len(), 2);

    let home = routes.iter().find(|r| r.full_path == "/").unwrap();
    assert!(home.component_path.is_some());
    assert!(home.is_lazy);

    let about = routes.iter().find(|r| r.full_path == "/about").unwrap();
    assert!(about.component_path.is_some());

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_file_based_dynamic_params() {
    let tmp = std::env::temp_dir().join("verter_test_dynamic_params");
    let pages = tmp.join("pages");
    let users = pages.join("users");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&users).unwrap();

    std::fs::write(users.join("[id].vue"), "<template>User</template>").unwrap();

    let routes = extract_file_based_routes(&pages);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].full_path, "/users/:id");
}

#[test]
fn test_file_based_catch_all() {
    let tmp = std::env::temp_dir().join("verter_test_catch_all");
    let pages = tmp.join("pages");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&pages).unwrap();

    std::fs::write(pages.join("[...slug].vue"), "<template>404</template>").unwrap();

    let routes = extract_file_based_routes(&pages);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].full_path, "/:slug(.*)*");
}

#[test]
fn test_file_based_nonexistent_dir() {
    let routes = extract_file_based_routes(std::path::Path::new("/nonexistent/pages"));
    assert!(routes.is_empty());
}

#[test]
fn test_file_based_ignores_non_vue() {
    let tmp = std::env::temp_dir().join("verter_test_non_vue");
    let pages = tmp.join("pages");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&pages).unwrap();

    std::fs::write(pages.join("index.vue"), "<template>Home</template>").unwrap();
    std::fs::write(pages.join("utils.ts"), "export const x = 1").unwrap();
    std::fs::write(pages.join("README.md"), "# Pages").unwrap();

    let routes = extract_file_based_routes(&pages);

    assert_eq!(routes.len(), 1, "should only include .vue files");
    assert_eq!(routes[0].full_path, "/");

    let _ = std::fs::remove_dir_all(&tmp);
}

// =============================================================================
// Build Full Path Tests
// =============================================================================

#[test]
fn test_build_full_path_root() {
    assert_eq!(build_full_path("", "/"), "/");
    assert_eq!(build_full_path("", "about"), "/about");
}

#[test]
fn test_build_full_path_nested() {
    assert_eq!(build_full_path("/admin", "users"), "/admin/users");
    assert_eq!(build_full_path("/admin/", "users"), "/admin/users");
}

#[test]
fn test_build_full_path_absolute_child() {
    // Absolute child paths override parent
    assert_eq!(build_full_path("/admin", "/dashboard"), "/dashboard");
}

#[test]
fn test_build_full_path_empty_child() {
    assert_eq!(build_full_path("/admin", ""), "/admin");
}

// =============================================================================
// Convert Param Syntax Tests
// =============================================================================

#[test]
fn test_convert_dynamic_param() {
    assert_eq!(convert_param_syntax("[id]"), ":id");
    assert_eq!(convert_param_syntax("[slug]"), ":slug");
}

#[test]
fn test_convert_catch_all() {
    assert_eq!(convert_param_syntax("[...all]"), ":all(.*)*");
    assert_eq!(convert_param_syntax("[...slug]"), ":slug(.*)*");
}

#[test]
fn test_convert_static_name() {
    assert_eq!(convert_param_syntax("users"), "users");
    assert_eq!(convert_param_syntax("about"), "about");
}

// =============================================================================
// Route Guard Extraction Tests
// =============================================================================

#[test]
fn test_extract_composition_guards() {
    let content = r#"
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router'

onBeforeRouteLeave((to, from) => {
  return false
})

onBeforeRouteUpdate((to) => {
  console.log(to)
})
"#;

    let guards = extract_route_guards(content, "component.ts");

    assert_eq!(guards.len(), 2);
    assert_eq!(guards[0].kind, RouteGuardKind::OnBeforeRouteLeave);
    assert_eq!(guards[1].kind, RouteGuardKind::OnBeforeRouteUpdate);
}

#[test]
fn test_extract_global_guards() {
    let content = r#"
router.beforeEach((to, from) => {
  if (!isAuthenticated) return '/login'
})

router.afterEach((to, from) => {
  document.title = to.meta.title
})
"#;

    let guards = extract_route_guards(content, "router.ts");

    assert_eq!(guards.len(), 2);
    assert!(
        guards
            .iter()
            .all(|g| g.kind == RouteGuardKind::NavigationGuard),
        "global guards should be NavigationGuard kind"
    );
}

#[test]
fn test_extract_no_guards() {
    let content = r#"
const x = ref(0)
console.log('hello')
"#;

    let guards = extract_route_guards(content, "component.ts");
    assert!(guards.is_empty(), "should find no guards in non-route code");
}

// =============================================================================
// Route Health Analysis Tests
// =============================================================================

#[test]
fn test_health_missing_components() {
    let snapshot = RouteAnalysisSnapshot {
        framework: RoutingFramework::VueRouter,
        routes: vec![RouteDefinition {
            path: "/".to_string(),
            full_path: "/".to_string(),
            name: None,
            component_path: Some("./views/Missing.vue".to_string()),
            is_lazy: true,
            redirect: None,
            meta: Vec::new(),
            children: Vec::new(),
            guards: Vec::new(),
            source_span: None,
        }],
        navigation_links: Vec::new(),
        layouts: Vec::new(),
        router_view_locations: Vec::new(),
        global_guards: Vec::new(),
    };

    let existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    let report = analyze_route_health(&snapshot, &existing);

    assert_eq!(report.missing_components.len(), 1);
    assert!(report.missing_components[0].detail.contains("Missing.vue"));
}

#[test]
fn test_health_dead_routes() {
    let snapshot = RouteAnalysisSnapshot {
        framework: RoutingFramework::VueRouter,
        routes: vec![
            RouteDefinition {
                path: "/".to_string(),
                full_path: "/".to_string(),
                name: Some("home".to_string()),
                component_path: Some("./Home.vue".to_string()),
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
            RouteDefinition {
                path: "/orphan".to_string(),
                full_path: "/orphan".to_string(),
                name: None,
                component_path: Some("./Orphan.vue".to_string()),
                is_lazy: true,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
        ],
        navigation_links: Vec::new(),
        layouts: Vec::new(),
        router_view_locations: Vec::new(),
        global_guards: Vec::new(),
    };

    let existing: std::collections::HashSet<String> = ["./Home.vue", "./Orphan.vue"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let report = analyze_route_health(&snapshot, &existing);

    // "/" is excluded from dead routes check, but "/orphan" has no links
    assert_eq!(report.dead_routes.len(), 1);
    assert_eq!(report.dead_routes[0].route_path, "/orphan");
    // "/" should NOT be in dead_routes
    assert!(
        !report.dead_routes.iter().any(|r| r.route_path == "/"),
        "root route should be excluded from dead route check"
    );
}

#[test]
fn test_health_duplicate_paths() {
    let snapshot = RouteAnalysisSnapshot {
        framework: RoutingFramework::VueRouter,
        routes: vec![
            RouteDefinition {
                path: "/about".to_string(),
                full_path: "/about".to_string(),
                name: Some("about1".to_string()),
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
            RouteDefinition {
                path: "/about".to_string(),
                full_path: "/about".to_string(),
                name: Some("about2".to_string()),
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
        ],
        navigation_links: Vec::new(),
        layouts: Vec::new(),
        router_view_locations: Vec::new(),
        global_guards: Vec::new(),
    };

    let report = analyze_route_health(&snapshot, &std::collections::HashSet::new());
    assert!(
        report.duplicate_paths.contains(&"/about".to_string()),
        "should detect duplicate paths"
    );
}

#[test]
fn test_health_duplicate_names() {
    let snapshot = RouteAnalysisSnapshot {
        framework: RoutingFramework::VueRouter,
        routes: vec![
            RouteDefinition {
                path: "/a".to_string(),
                full_path: "/a".to_string(),
                name: Some("page".to_string()),
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
            RouteDefinition {
                path: "/b".to_string(),
                full_path: "/b".to_string(),
                name: Some("page".to_string()),
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
        ],
        navigation_links: Vec::new(),
        layouts: Vec::new(),
        router_view_locations: Vec::new(),
        global_guards: Vec::new(),
    };

    let report = analyze_route_health(&snapshot, &std::collections::HashSet::new());
    assert!(
        report.duplicate_names.contains(&"page".to_string()),
        "should detect duplicate names"
    );
}

// =============================================================================
// Flatten Routes Tests
// =============================================================================

#[test]
fn test_flatten_routes() {
    let routes = vec![RouteDefinition {
        path: "/admin".to_string(),
        full_path: "/admin".to_string(),
        name: None,
        component_path: None,
        is_lazy: false,
        redirect: None,
        meta: Vec::new(),
        children: vec![
            RouteDefinition {
                path: "users".to_string(),
                full_path: "/admin/users".to_string(),
                name: None,
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
            RouteDefinition {
                path: "settings".to_string(),
                full_path: "/admin/settings".to_string(),
                name: None,
                component_path: None,
                is_lazy: false,
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            },
        ],
        guards: Vec::new(),
        source_span: None,
    }];

    let flat = flatten_routes(&routes);
    assert_eq!(flat.len(), 3);
    assert_eq!(flat[0].full_path, "/admin");
    assert_eq!(flat[1].full_path, "/admin/users");
    assert_eq!(flat[2].full_path, "/admin/settings");
}

// =============================================================================
// CoreUI-style Route Extraction Test (real-world pattern)
// =============================================================================

#[test]
fn test_extract_coreui_style_routes() {
    // CoreUI pattern: `const routes = [...]` then `createRouter({ routes })`
    let content = r#"
import { createRouter, createWebHashHistory } from 'vue-router'
import DefaultLayout from '@/layouts/DefaultLayout'

const routes = [
  {
    path: '/',
    name: 'Home',
    component: DefaultLayout,
    redirect: '/dashboard',
    children: [
      {
        path: '/dashboard',
        name: 'Dashboard',
        component: () => import('@/views/dashboard/Dashboard.vue'),
      },
      {
        path: '/theme/colors',
        name: 'Colors',
        component: () => import('@/views/theme/Colors.vue'),
      },
    ],
  },
  {
    path: '/pages',
    redirect: '/pages/404',
    name: 'Pages',
    children: [
      {
        path: '404',
        name: 'Page404',
        component: () => import('@/views/pages/Page404'),
      },
    ],
  },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
"#;

    let project_root = std::path::Path::new("/project");
    let routes = extract_programmatic_routes(content, "router.js", project_root);

    // Should extract routes from `const routes = [...]` (not from createRouter since it uses a variable ref)
    assert!(
        routes.len() >= 2,
        "should extract at least 2 top-level routes, got {}",
        routes.len()
    );

    // Check Home route
    let home = routes.iter().find(|r| r.path == "/");
    assert!(home.is_some(), "should have root route /");
    let home = home.unwrap();
    assert_eq!(home.name.as_deref(), Some("Home"));
    assert_eq!(home.redirect.as_deref(), Some("/dashboard"));
    assert_eq!(
        home.component_path.as_deref(),
        Some("@/layouts/DefaultLayout"),
        "eager import should resolve to import source"
    );
    assert!(!home.is_lazy, "eager import should not be lazy");

    // Check nested children
    assert!(
        home.children.len() >= 2,
        "Home should have children, got {}",
        home.children.len()
    );
    let dashboard = home.children.iter().find(|r| r.name.as_deref() == Some("Dashboard"));
    assert!(dashboard.is_some(), "should have Dashboard child route");
    let dashboard = dashboard.unwrap();
    assert_eq!(dashboard.full_path, "/dashboard");
    assert!(dashboard.is_lazy, "lazy import should be marked lazy");

    // Check Pages route with relative child paths
    let pages = routes.iter().find(|r| r.path == "/pages");
    assert!(pages.is_some(), "should have /pages route");
    let pages = pages.unwrap();
    assert_eq!(pages.children.len(), 1);
    assert_eq!(pages.children[0].path, "404");
    assert_eq!(pages.children[0].full_path, "/pages/404");

    // Negative assertions
    assert!(
        !routes.iter().any(|r| r.path.contains("createRouter")),
        "should not have createRouter as a route path"
    );
}

// =============================================================================
// Serialization Tests
// =============================================================================

#[test]
fn test_snapshot_serializes() {
    let snapshot = RouteAnalysisSnapshot::default();
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains("\"framework\""));
    assert!(json.contains("\"routes\""));
    assert!(json.contains("\"navigationLinks\""));
    // Verify camelCase serialization
    assert!(!json.contains("navigation_links"), "should use camelCase");
    assert!(!json.contains("router_view"), "should use camelCase");
}

#[test]
fn test_route_definition_round_trip() {
    let route = RouteDefinition {
        path: "/users/:id".to_string(),
        full_path: "/users/:id".to_string(),
        name: Some("user-detail".to_string()),
        component_path: Some("./views/UserDetail.vue".to_string()),
        is_lazy: true,
        redirect: None,
        meta: vec![("requiresAuth".to_string(), "true".to_string())],
        children: Vec::new(),
        guards: Vec::new(),
        source_span: None,
    };

    let json = serde_json::to_string(&route).unwrap();
    let deserialized: RouteDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.path, "/users/:id");
    assert_eq!(deserialized.name.as_deref(), Some("user-detail"));
    assert!(deserialized.is_lazy);
}
