//! Route analysis for Vue applications.
//!
//! Extracts routing structure from:
//! - Programmatic route configs (`createRouter({ routes: [...] })`)
//! - File-based routing (`pages/` directory conventions)
//! - Navigation links (`<RouterLink>`, `<NuxtLink>`)
//! - Router views (`<RouterView>`, `<NuxtPage>`)
//! - Layouts (`layouts/` directory)
//! - Route guards (global, per-route, in-component)

use oxc_ast::ast::{
    ArrayExpressionElement, BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression,
    ImportDeclarationSpecifier, ObjectPropertyKind, Statement,
};
use serde::{Deserialize, Serialize};
use verter_span::Span;

// =============================================================================
// Core Types
// =============================================================================

/// Which routing framework the project uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoutingFramework {
    VueRouter,
    NuxtPages,
    UnpluginVueRouter,
    Unknown,
}

/// A single route definition extracted from router config or file-based routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDefinition {
    /// Route path pattern, e.g. "/users/:id"
    pub path: String,
    /// Full resolved path including parent prefixes, e.g. "/admin/users/:id"
    pub full_path: String,
    /// Named route identifier
    pub name: Option<String>,
    /// Resolved file path to the .vue component
    pub component_path: Option<String>,
    /// Whether the component is lazy-loaded via `() => import(...)`
    pub is_lazy: bool,
    /// Redirect target path
    pub redirect: Option<String>,
    /// Static meta key-value pairs
    pub meta: Vec<(String, String)>,
    /// Nested child routes
    pub children: Vec<RouteDefinition>,
    /// Per-route guards (beforeEnter, etc.)
    pub guards: Vec<RouteGuard>,
    /// Source location in the router config file
    pub source_span: Option<Span>,
}

/// A route guard (navigation hook).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteGuard {
    pub kind: RouteGuardKind,
    pub file_path: String,
    pub span: Span,
}

/// The kind of route guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteGuardKind {
    /// `beforeEnter` on a route definition
    BeforeEnter,
    /// Options API `beforeRouteEnter`
    BeforeRouteEnter,
    /// Options API `beforeRouteLeave`
    BeforeRouteLeave,
    /// Composition API `onBeforeRouteLeave()`
    OnBeforeRouteLeave,
    /// Composition API `onBeforeRouteUpdate()`
    OnBeforeRouteUpdate,
    /// Global `router.beforeEach()` / `router.afterEach()`
    NavigationGuard,
}

/// A navigation link found in a template (`<RouterLink>`, `<NuxtLink>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationLink {
    pub target: NavigationTarget,
    pub file_path: String,
    pub span: Span,
}

/// The target of a navigation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NavigationTarget {
    /// Static path string, e.g. `to="/about"`
    Path(String),
    /// Named route, e.g. `:to="{ name: 'user-detail' }"`
    NamedRoute(String),
    /// Dynamic expression that can't be statically resolved
    Dynamic,
}

/// A `<RouterView>` or `<NuxtPage>` usage found in a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterViewLocation {
    pub file_path: String,
    pub span: Span,
    /// Named view, e.g. `<RouterView name="sidebar" />`
    pub name: Option<String>,
}

/// A layout definition (Nuxt `layouts/` directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDefinition {
    pub name: String,
    pub file_path: String,
    pub is_default: bool,
}

/// Complete route analysis result for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAnalysisSnapshot {
    pub framework: RoutingFramework,
    pub routes: Vec<RouteDefinition>,
    pub navigation_links: Vec<NavigationLink>,
    pub layouts: Vec<LayoutDefinition>,
    pub router_view_locations: Vec<RouterViewLocation>,
    pub global_guards: Vec<RouteGuard>,
}

impl Default for RouteAnalysisSnapshot {
    fn default() -> Self {
        Self {
            framework: RoutingFramework::Unknown,
            routes: Vec::new(),
            navigation_links: Vec::new(),
            layouts: Vec::new(),
            router_view_locations: Vec::new(),
            global_guards: Vec::new(),
        }
    }
}

/// Result of route health analysis (cross-referencing routes with components).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteHealthReport {
    /// Routes whose component_path points to a file that doesn't exist
    pub missing_components: Vec<RouteHealthIssue>,
    /// Routes that no NavigationLink points to
    pub dead_routes: Vec<RouteHealthIssue>,
    /// Components containing `<RouterView>` but not in the route tree
    pub orphan_views: Vec<String>,
    /// Layout names referenced in routes but not found in layouts/
    pub missing_layouts: Vec<String>,
    /// Duplicate route paths
    pub duplicate_paths: Vec<String>,
    /// Duplicate route names
    pub duplicate_names: Vec<String>,
    /// Guard coverage statistics
    pub guard_coverage: Option<GuardCoverage>,
}

/// Statistics about route guard coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardCoverage {
    /// Total number of routes (flattened)
    pub total_routes: usize,
    /// Number of routes with per-route guards (e.g. beforeEnter)
    pub routes_with_guards: usize,
    /// Whether any global navigation guard exists (beforeEach/afterEach/beforeResolve)
    pub has_global_guard: bool,
}

/// A single route health issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteHealthIssue {
    pub route_path: String,
    pub detail: String,
}

// =============================================================================
// Framework Detection
// =============================================================================

/// Detect the routing framework from `package.json` dependencies.
pub fn detect_routing_framework(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> RoutingFramework {
    let pkg_path = format!("{}/package.json", project_root.trim_end_matches('/'));
    let content = match workspace.read_file(&pkg_path) {
        Some(c) => c,
        None => return RoutingFramework::Unknown,
    };

    detect_routing_framework_from_json(&content)
}

/// Detect routing framework from package.json content string.
pub fn detect_routing_framework_from_json(content: &str) -> RoutingFramework {
    let json: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return RoutingFramework::Unknown,
    };

    let has_dep = |name: &str| -> bool {
        json.get("dependencies").and_then(|d| d.get(name)).is_some()
            || json
                .get("devDependencies")
                .and_then(|d| d.get(name))
                .is_some()
    };

    // Order matters: Nuxt implies vue-router internally, check it first
    if has_dep("nuxt") || has_dep("nuxt3") {
        RoutingFramework::NuxtPages
    } else if has_dep("unplugin-vue-router") {
        RoutingFramework::UnpluginVueRouter
    } else if has_dep("vue-router") {
        RoutingFramework::VueRouter
    } else {
        RoutingFramework::Unknown
    }
}

// =============================================================================
// Router Config File Discovery
// =============================================================================

/// Common router config file locations to search.
const ROUTER_CONFIG_CANDIDATES: &[&str] = &[
    "src/router/index.ts",
    "src/router/index.js",
    "src/router.ts",
    "src/router.js",
    "router/index.ts",
    "router/index.js",
];

/// Discover router config files in a project.
pub fn discover_router_configs(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> Vec<String> {
    let trimmed = project_root.trim_end_matches('/');
    ROUTER_CONFIG_CANDIDATES
        .iter()
        .map(|candidate| format!("{}/{}", trimmed, candidate))
        .filter(|p| workspace.file_exists(p))
        .collect()
}

// =============================================================================
// Programmatic Route Extraction (OXC-based)
// =============================================================================

/// Extract route definitions from a programmatic router config file.
///
/// Parses the file content with OXC and walks the AST looking for:
/// - `createRouter({ routes: [...] })` calls
/// - `export const routes = [...]` or `const routes = [...]` declarations
/// - Route object literals with `path`, `name`, `component`, `children`, etc.
pub fn extract_programmatic_routes(
    content: &str,
    file_path: &str,
    project_root: &std::path::Path,
) -> Vec<RouteDefinition> {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = if file_path.ends_with(".ts") || file_path.ends_with(".tsx") {
        oxc_span::SourceType::ts()
    } else {
        oxc_span::SourceType::mjs()
    };
    let parser_ret = oxc_parser::Parser::new(&allocator, content, source_type).parse();
    let program = &parser_ret.program;

    // Collect import map: local_name -> source (for resolving eager component imports)
    let import_map = build_import_map(program);

    // Find route arrays in the AST
    let mut routes = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                if let Some(found) =
                    find_create_router_routes(&expr_stmt.expression, &import_map, project_root)
                {
                    routes.extend(found);
                }
            }
            Statement::VariableDeclaration(decl) => {
                extract_routes_from_var_decl(decl, &import_map, project_root, &mut routes);
            }
            Statement::ExportDefaultDeclaration(export_default) => {
                // export default createRouter({ routes: [...] })
                if let ExportDefaultDeclarationKind::CallExpression(call) =
                    &export_default.declaration
                {
                    if let Expression::Identifier(callee_id) = &call.callee {
                        if callee_id.name.as_str() == "createRouter" {
                            if let Some(first_arg) = call.arguments.first() {
                                if let Some(Expression::ObjectExpression(obj)) =
                                    first_arg.as_expression()
                                {
                                    for prop in &obj.properties {
                                        if let ObjectPropertyKind::ObjectProperty(p) = prop {
                                            if p.key.name().as_deref() == Some("routes") {
                                                if let Expression::ArrayExpression(arr) = &p.value {
                                                    routes.extend(extract_routes_from_array(
                                                        arr,
                                                        "",
                                                        &import_map,
                                                        project_root,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Statement::ExportNamedDeclaration(export_named) => {
                if let Some(Declaration::VariableDeclaration(var_decl)) = &export_named.declaration
                {
                    extract_routes_from_var_decl(var_decl, &import_map, project_root, &mut routes);
                }
            }
            _ => {}
        }
    }

    routes
}

/// Build a map of import local names to their source paths.
fn build_import_map(
    program: &oxc_ast::ast::Program<'_>,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for stmt in &program.body {
        if let Statement::ImportDeclaration(import) = stmt {
            let source = import.source.value.as_str().to_string();
            if let Some(specifiers) = &import.specifiers {
                for spec in specifiers {
                    match spec {
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            map.insert(s.local.name.to_string(), source.clone());
                        }
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            map.insert(s.local.name.to_string(), source.clone());
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            map.insert(s.local.name.to_string(), source.clone());
                        }
                    }
                }
            }
        }
    }
    map
}

/// Extract routes from a variable declaration (handles `const routes = [...]` and
/// `const router = createRouter({ routes: [...] })`).
fn extract_routes_from_var_decl(
    decl: &oxc_ast::ast::VariableDeclaration<'_>,
    import_map: &std::collections::HashMap<String, String>,
    project_root: &std::path::Path,
    routes: &mut Vec<RouteDefinition>,
) {
    for declarator in &decl.declarations {
        // Check for `const routes = [...]`
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            if id.name.as_str() == "routes" {
                if let Some(Expression::ArrayExpression(arr)) = &declarator.init {
                    routes.extend(extract_routes_from_array(arr, "", import_map, project_root));
                }
            }
        }
        // Also check for `const router = createRouter({ routes: [...] })`
        if let Some(init) = &declarator.init {
            if let Some(found) = find_create_router_routes(init, import_map, project_root) {
                routes.extend(found);
            }
        }
    }
}

/// Look for `createRouter({ routes: [...] })` in an expression and extract routes.
fn find_create_router_routes(
    expr: &Expression<'_>,
    import_map: &std::collections::HashMap<String, String>,
    project_root: &std::path::Path,
) -> Option<Vec<RouteDefinition>> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let Expression::Identifier(callee_id) = &call.callee else {
        return None;
    };
    if callee_id.name.as_str() != "createRouter" {
        return None;
    }
    // First argument should be an object with a `routes` property
    let first_arg = call.arguments.first()?;
    let Expression::ObjectExpression(obj) = first_arg.as_expression()? else {
        return None;
    };
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if p.key.name().as_deref() == Some("routes") {
                if let Expression::ArrayExpression(arr) = &p.value {
                    return Some(extract_routes_from_array(arr, "", import_map, project_root));
                }
            }
        }
    }
    None
}

/// Extract route definitions from an array expression.
fn extract_routes_from_array(
    arr: &oxc_ast::ast::ArrayExpression<'_>,
    parent_path: &str,
    import_map: &std::collections::HashMap<String, String>,
    project_root: &std::path::Path,
) -> Vec<RouteDefinition> {
    let mut routes = Vec::new();
    for element in &arr.elements {
        if let ArrayExpressionElement::ObjectExpression(obj) = element {
            if let Some(route) =
                extract_route_from_object(obj, parent_path, import_map, project_root)
            {
                routes.push(route);
            }
        }
    }
    routes
}

/// Extract a single route definition from an object expression.
fn extract_route_from_object(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    parent_path: &str,
    import_map: &std::collections::HashMap<String, String>,
    _project_root: &std::path::Path,
) -> Option<RouteDefinition> {
    let mut path = String::new();
    let mut name = None;
    let mut component_path = None;
    let mut is_lazy = false;
    let mut redirect = None;
    let mut meta = Vec::new();
    let mut children = Vec::new();
    let mut guards = Vec::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_name = p.key.name();
        let Some(key) = key_name.as_deref() else {
            continue;
        };
        match key {
            "path" => {
                if let Some(s) = extract_string_literal(&p.value) {
                    path = s;
                }
            }
            "name" => {
                if let Some(s) = extract_string_literal(&p.value) {
                    name = Some(s);
                }
            }
            "component" => {
                // Eager: `component: MyComponent` (identifier referencing import)
                if let Expression::Identifier(ident) = &p.value {
                    if let Some(source) = import_map.get(ident.name.as_str()) {
                        component_path = Some(resolve_component_path(source));
                        is_lazy = false;
                    }
                }
                // Lazy: `component: () => import('./views/Home.vue')`
                if let Expression::ArrowFunctionExpression(arrow) = &p.value {
                    if let Some(import_path) = extract_dynamic_import_from_arrow(arrow) {
                        component_path = Some(resolve_component_path(&import_path));
                        is_lazy = true;
                    }
                }
            }
            "redirect" => {
                if let Some(s) = extract_string_literal(&p.value) {
                    redirect = Some(s);
                }
            }
            "meta" => {
                if let Expression::ObjectExpression(obj_expr) = &p.value {
                    meta = extract_meta_pairs(obj_expr);
                }
            }
            "children" => {
                if let Expression::ArrayExpression(arr) = &p.value {
                    let full = build_full_path(parent_path, &path);
                    children = extract_routes_from_array(arr, &full, import_map, _project_root);
                }
            }
            "beforeEnter" => {
                guards.push(RouteGuard {
                    kind: RouteGuardKind::BeforeEnter,
                    file_path: String::new(),
                    span: Span::new(p.span.start, p.span.end),
                });
            }
            _ => {}
        }
    }

    // path is required for a valid route
    if path.is_empty() && redirect.is_none() {
        return None;
    }

    let full_path = build_full_path(parent_path, &path);

    Some(RouteDefinition {
        path,
        full_path,
        name,
        component_path,
        is_lazy,
        redirect,
        meta,
        children,
        guards,
        source_span: Some(Span::new(obj.span.start, obj.span.end)),
    })
}

/// Extract a string literal value from an expression.
fn extract_string_literal(expr: &Expression<'_>) -> Option<String> {
    if let Expression::StringLiteral(s) = expr {
        Some(s.value.to_string())
    } else {
        None
    }
}

/// Extract the import path from an arrow function containing a dynamic import.
fn extract_dynamic_import_from_arrow(
    arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
) -> Option<String> {
    // Arrow functions: check all statements in body
    // Expression body `() => import('./path')` is desugared to a single ExpressionStatement
    for stmt in &arrow.body.statements {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                if let Some(path) = extract_dynamic_import_path(&expr_stmt.expression) {
                    return Some(path);
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    if let Some(path) = extract_dynamic_import_path(arg) {
                        return Some(path);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract the path string from an `import('...')` expression.
fn extract_dynamic_import_path(expr: &Expression<'_>) -> Option<String> {
    if let Expression::ImportExpression(import_expr) = expr {
        extract_string_literal(&import_expr.source)
    } else {
        None
    }
}

/// Extract static key-value pairs from a meta object expression.
fn extract_meta_pairs(obj: &oxc_ast::ast::ObjectExpression<'_>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let Some(key) = p.key.name() {
                if let Some(val) = extract_string_literal(&p.value) {
                    pairs.push((key.to_string(), val));
                } else if let Expression::BooleanLiteral(bool_lit) = &p.value {
                    pairs.push((key.to_string(), bool_lit.value.to_string()));
                } else if let Expression::NumericLiteral(num_lit) = &p.value {
                    let raw_str = num_lit
                        .raw
                        .as_ref()
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| num_lit.value.to_string());
                    pairs.push((key.to_string(), raw_str));
                }
            }
        }
    }
    pairs
}

/// Build a full route path from parent prefix and child path.
fn build_full_path(parent: &str, child: &str) -> String {
    if child.starts_with('/') {
        // Absolute child path overrides parent
        child.to_string()
    } else if parent.is_empty() {
        format!("/{}", child.trim_start_matches('/'))
    } else {
        let parent = parent.trim_end_matches('/');
        if child.is_empty() {
            parent.to_string()
        } else {
            format!("{}/{}", parent, child)
        }
    }
}

/// Resolve a component import path (placeholder — returns as-is for now).
fn resolve_component_path(import_source: &str) -> String {
    import_source.to_string()
}

// =============================================================================
// File-Based Route Extraction
// =============================================================================

/// Extract routes from a file-based routing directory (e.g., `pages/`).
///
/// Applies Nuxt/unplugin-vue-router conventions:
/// - `index.vue` → `/`
/// - `about.vue` → `/about`
/// - `[param].vue` → `/:param`
/// - `[...slug].vue` → `/:slug(.*)*`
/// - Directory nesting → nested route paths
pub fn extract_file_based_routes(
    workspace: &dyn verter_workspace::WorkspaceRead,
    pages_dir: &str,
) -> Vec<RouteDefinition> {
    if !workspace.is_dir(pages_dir) {
        return Vec::new();
    }

    extract_file_routes_recursive(workspace, pages_dir, "")
}

fn extract_file_routes_recursive(
    workspace: &dyn verter_workspace::WorkspaceRead,
    current_dir: &str,
    parent_path: &str,
) -> Vec<RouteDefinition> {
    let mut routes = Vec::new();

    let Ok(entries) = workspace.read_dir(current_dir) else {
        return routes;
    };

    let mut entries = entries;
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    for entry in entries {
        // Extract the basename from the canonical path
        let name_str = entry.path.rsplit('/').next().unwrap_or(entry.path.as_str());

        if entry.is_dir {
            // Recurse into subdirectory
            let segment = dir_name_to_route_segment(name_str);
            let dir_path = if parent_path.is_empty() {
                format!("/{}", segment)
            } else {
                format!("{}/{}", parent_path.trim_end_matches('/'), segment)
            };
            let children = extract_file_routes_recursive(workspace, &entry.path, &dir_path);
            routes.extend(children);
        } else {
            let path = std::path::Path::new(name_str);
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();

            if ext != "vue" {
                continue;
            }

            let segment = file_name_to_route_segment(&stem);
            let route_path = if segment.is_empty() {
                // index.vue → parent path (or /)
                if parent_path.is_empty() {
                    "/".to_string()
                } else {
                    parent_path.to_string()
                }
            } else if parent_path.is_empty() {
                format!("/{}", segment)
            } else {
                format!("{}/{}", parent_path.trim_end_matches('/'), segment)
            };

            let component = entry.path.clone();

            routes.push(RouteDefinition {
                path: segment,
                full_path: route_path,
                name: None,
                component_path: Some(component),
                is_lazy: true, // file-based routes are always lazy
                redirect: None,
                meta: Vec::new(),
                children: Vec::new(),
                guards: Vec::new(),
                source_span: None,
            });
        }
    }

    routes
}

/// Convert a directory name to a route segment.
fn dir_name_to_route_segment(name: &str) -> String {
    convert_param_syntax(name)
}

/// Convert a file name (without extension) to a route segment.
fn file_name_to_route_segment(stem: &str) -> String {
    if stem == "index" {
        return String::new();
    }
    convert_param_syntax(stem)
}

/// Convert Nuxt-style `[param]` and `[...slug]` to vue-router `:param` and `:slug(.*)*`.
fn convert_param_syntax(name: &str) -> String {
    if name.starts_with("[...") && name.ends_with(']') {
        // Catch-all: [...slug] → :slug(.*)*
        let param = &name[4..name.len() - 1];
        format!(":{}(.*)*", param)
    } else if name.starts_with('[') && name.ends_with(']') {
        // Dynamic param: [id] → :id
        let param = &name[1..name.len() - 1];
        format!(":{}", param)
    } else {
        name.to_string()
    }
}

// =============================================================================
// Navigation Link Extraction
// =============================================================================

/// Router-related component names to detect.
const ROUTER_LINK_NAMES: &[&str] = &["RouterLink", "router-link", "NuxtLink", "nuxt-link"];
const ROUTER_VIEW_NAMES: &[&str] = &["RouterView", "router-view", "NuxtPage", "nuxt-page"];

/// Extract navigation links from template component usages.
pub fn extract_navigation_links(
    components: &[crate::analysis::template::TemplateComponentUsage],
    file_path: &str,
) -> Vec<NavigationLink> {
    let mut links = Vec::new();

    for comp in components {
        let is_link = ROUTER_LINK_NAMES
            .iter()
            .any(|n| comp.name.eq_ignore_ascii_case(n));
        if !is_link {
            continue;
        }

        let target = extract_nav_target_from_props(&comp.props);
        links.push(NavigationLink {
            target,
            file_path: file_path.to_string(),
            span: comp.span,
        });
    }

    links
}

/// Extract navigation target from a component's props.
fn extract_nav_target_from_props(
    props: &[crate::analysis::template::TemplatePropUsage],
) -> NavigationTarget {
    for prop in props {
        if prop.name == "to" {
            if !prop.is_bound {
                // Static: to="/about"
                return NavigationTarget::Path(prop.name.clone());
            }
            // Bound: :to="..." — check referenced bindings for hints
            // For now, mark as Dynamic since we can't resolve runtime expressions
            return NavigationTarget::Dynamic;
        }
    }
    NavigationTarget::Dynamic
}

/// Extract RouterView/NuxtPage locations from template component usages.
pub fn extract_router_views(
    components: &[crate::analysis::template::TemplateComponentUsage],
    file_path: &str,
) -> Vec<RouterViewLocation> {
    let mut views = Vec::new();

    for comp in components {
        let is_view = ROUTER_VIEW_NAMES
            .iter()
            .any(|n| comp.name.eq_ignore_ascii_case(n));
        if !is_view {
            continue;
        }

        let name = comp.props.iter().find(|p| p.name == "name").and_then(|p| {
            if !p.is_bound {
                // We need to get the static value - the referenced_bindings won't help here
                // For static props, the value isn't in TemplatePropUsage directly
                // Mark as named but with unknown name for now
                Some(p.name.clone())
            } else {
                None
            }
        });

        views.push(RouterViewLocation {
            file_path: file_path.to_string(),
            span: comp.span,
            name,
        });
    }

    views
}

// =============================================================================
// Layout Discovery
// =============================================================================

/// Discover layout definitions from the `layouts/` directory.
pub fn discover_layouts(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> Vec<LayoutDefinition> {
    let layouts_dir = format!("{}/layouts", project_root.trim_end_matches('/'));
    if !workspace.is_dir(&layouts_dir) {
        return Vec::new();
    }

    let mut layouts = Vec::new();
    let Ok(entries) = workspace.read_dir(&layouts_dir) else {
        return layouts;
    };

    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let name_str = entry.path.rsplit('/').next().unwrap_or(entry.path.as_str());
        let path = std::path::Path::new(name_str);
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        if ext != "vue" {
            continue;
        }

        let is_default = stem == "default";
        layouts.push(LayoutDefinition {
            name: stem,
            file_path: entry.path.clone(),
            is_default,
        });
    }

    layouts.sort_by(|a, b| a.name.cmp(&b.name));
    layouts
}

// =============================================================================
// Route Guard Extraction
// =============================================================================

/// Extract route guards from script content.
pub fn extract_route_guards(content: &str, file_path: &str) -> Vec<RouteGuard> {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = if file_path.ends_with(".ts") || file_path.ends_with(".tsx") {
        oxc_span::SourceType::ts()
    } else {
        oxc_span::SourceType::mjs()
    };
    let parser_ret = oxc_parser::Parser::new(&allocator, content, source_type).parse();
    let program = &parser_ret.program;

    let mut guards = Vec::new();

    for stmt in &program.body {
        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            extract_guards_from_expr(&expr_stmt.expression, file_path, &mut guards);
        }
    }

    guards
}

fn extract_guards_from_expr(expr: &Expression<'_>, file_path: &str, guards: &mut Vec<RouteGuard>) {
    let Expression::CallExpression(call) = expr else {
        return;
    };

    // Direct call: onBeforeRouteLeave(() => { ... })
    if let Expression::Identifier(ident) = &call.callee {
        let kind = match ident.name.as_str() {
            "onBeforeRouteLeave" => Some(RouteGuardKind::OnBeforeRouteLeave),
            "onBeforeRouteUpdate" => Some(RouteGuardKind::OnBeforeRouteUpdate),
            _ => None,
        };
        if let Some(kind) = kind {
            guards.push(RouteGuard {
                kind,
                file_path: file_path.to_string(),
                span: Span::new(call.span.start, call.span.end),
            });
        }
    }

    // Member call: router.beforeEach(...), router.afterEach(...)
    if let Some(member) = call.callee.as_member_expression() {
        if let Some(name) = member.static_property_name() {
            let kind = match name {
                "beforeEach" | "afterEach" | "beforeResolve" => {
                    Some(RouteGuardKind::NavigationGuard)
                }
                _ => None,
            };
            if let Some(kind) = kind {
                guards.push(RouteGuard {
                    kind,
                    file_path: file_path.to_string(),
                    span: Span::new(call.span.start, call.span.end),
                });
            }
        }
    }
}

// =============================================================================
// Route Health Analysis
// =============================================================================

/// Analyze route health by cross-referencing routes, components, and navigation.
pub fn analyze_route_health(
    snapshot: &RouteAnalysisSnapshot,
    existing_files: &std::collections::HashSet<String>,
) -> RouteHealthReport {
    let mut report = RouteHealthReport::default();

    let all_routes = flatten_routes(&snapshot.routes);

    // Check for missing components
    for route in &all_routes {
        if let Some(comp) = &route.component_path {
            if !existing_files.contains(comp) {
                report.missing_components.push(RouteHealthIssue {
                    route_path: route.full_path.clone(),
                    detail: format!("Component not found: {}", comp),
                });
            }
        }
    }

    // Check for dead routes (no navigation links pointing to them)
    let linked_paths: std::collections::HashSet<&str> = snapshot
        .navigation_links
        .iter()
        .filter_map(|link| match &link.target {
            NavigationTarget::Path(p) => Some(p.as_str()),
            _ => None,
        })
        .collect();

    let linked_names: std::collections::HashSet<&str> = snapshot
        .navigation_links
        .iter()
        .filter_map(|link| match &link.target {
            NavigationTarget::NamedRoute(n) => Some(n.as_str()),
            _ => None,
        })
        .collect();

    for route in &all_routes {
        let has_link_by_path = linked_paths.contains(route.full_path.as_str());
        let has_link_by_name = route
            .name
            .as_deref()
            .is_some_and(|n| linked_names.contains(n));
        if !has_link_by_path && !has_link_by_name && route.full_path != "/" {
            report.dead_routes.push(RouteHealthIssue {
                route_path: route.full_path.clone(),
                detail: "No navigation links point to this route".to_string(),
            });
        }
    }

    // Check for orphan views
    let route_components: std::collections::HashSet<&str> = all_routes
        .iter()
        .filter_map(|r| r.component_path.as_deref())
        .collect();

    for view in &snapshot.router_view_locations {
        if !route_components.contains(view.file_path.as_str()) {
            // This component has a RouterView but isn't referenced in any route
            report.orphan_views.push(view.file_path.clone());
        }
    }

    // Check for duplicate paths
    let mut path_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for route in &all_routes {
        *path_counts.entry(&route.full_path).or_insert(0) += 1;
    }
    for (path, count) in &path_counts {
        if *count > 1 {
            report.duplicate_paths.push(path.to_string());
        }
    }

    // Check for duplicate names
    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for route in &all_routes {
        if let Some(name) = &route.name {
            *name_counts.entry(name).or_insert(0) += 1;
        }
    }
    for (name, count) in &name_counts {
        if *count > 1 {
            report.duplicate_names.push(name.to_string());
        }
    }

    // Check for missing layouts: route meta `layout` keys that don't match any known layout
    let layout_names: std::collections::HashSet<&str> =
        snapshot.layouts.iter().map(|l| l.name.as_str()).collect();

    for route in &all_routes {
        for (key, value) in &route.meta {
            if key == "layout"
                && !layout_names.contains(value.as_str())
                && !report.missing_layouts.contains(value)
            {
                report.missing_layouts.push(value.clone());
            }
        }
    }

    // Guard coverage statistics
    let routes_with_guards = all_routes.iter().filter(|r| !r.guards.is_empty()).count();
    let has_global_guard = !snapshot.global_guards.is_empty();

    report.guard_coverage = Some(GuardCoverage {
        total_routes: all_routes.len(),
        routes_with_guards,
        has_global_guard,
    });

    report
}

/// Flatten a route tree into a flat list (including all nested children).
pub fn flatten_routes(routes: &[RouteDefinition]) -> Vec<&RouteDefinition> {
    let mut result = Vec::new();
    for route in routes {
        result.push(route);
        result.extend(flatten_routes(&route.children));
    }
    result
}

// =============================================================================
// Full Project Route Analysis
// =============================================================================

/// Build a complete route analysis snapshot for a project.
pub fn build_route_analysis(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
    template_components: &[(
        String,
        Vec<crate::analysis::template::TemplateComponentUsage>,
    )],
) -> RouteAnalysisSnapshot {
    let framework = detect_routing_framework(workspace, project_root);
    let trimmed_root = project_root.trim_end_matches('/');

    // Extract routes based on framework type
    let routes = match framework {
        RoutingFramework::NuxtPages => {
            let pages_dir = format!("{trimmed_root}/pages");
            extract_file_based_routes(workspace, &pages_dir)
        }
        RoutingFramework::UnpluginVueRouter => {
            // unplugin-vue-router uses file-based routing from src/pages/
            let pages_dir = format!("{trimmed_root}/src/pages");
            if workspace.is_dir(&pages_dir) {
                extract_file_based_routes(workspace, &pages_dir)
            } else {
                extract_file_based_routes(workspace, &format!("{trimmed_root}/pages"))
            }
        }
        RoutingFramework::VueRouter | RoutingFramework::Unknown => {
            // Try programmatic route extraction
            let configs = discover_router_configs(workspace, project_root);
            let mut all_routes = Vec::new();
            for config_path in configs {
                if let Some(content) = workspace.read_file(&config_path) {
                    all_routes.extend(extract_programmatic_routes(
                        &content,
                        &config_path,
                        std::path::Path::new(project_root),
                    ));
                }
            }
            all_routes
        }
    };

    // Extract navigation links and RouterView locations from template data
    let mut navigation_links = Vec::new();
    let mut router_view_locations = Vec::new();
    for (file_path, components) in template_components {
        navigation_links.extend(extract_navigation_links(components, file_path));
        router_view_locations.extend(extract_router_views(components, file_path));
    }

    // Discover layouts
    let layouts = discover_layouts(workspace, project_root);

    // Extract global guards from router config files
    let mut global_guards = Vec::new();
    for config_path in discover_router_configs(workspace, project_root) {
        if let Some(content) = workspace.read_file(&config_path) {
            global_guards.extend(extract_route_guards(&content, &config_path));
        }
    }

    RouteAnalysisSnapshot {
        framework,
        routes,
        navigation_links,
        layouts,
        router_view_locations,
        global_guards,
    }
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod routes_tests;
