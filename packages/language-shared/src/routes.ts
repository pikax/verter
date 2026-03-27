/** Which routing framework the project uses. */
export type RoutingFramework = "vueRouter" | "nuxtPages" | "unpluginVueRouter" | "unknown";

/** A single route definition extracted from router config or file-based routing. */
export interface RouteDefinition {
  path: string;
  fullPath: string;
  name?: string;
  componentPath?: string;
  isLazy: boolean;
  redirect?: string;
  meta: [string, string][];
  children: RouteDefinition[];
  guards: RouteGuard[];
  sourceSpan?: Span;
}

/** A route guard (navigation hook). */
export interface RouteGuard {
  kind: RouteGuardKind;
  filePath: string;
  span: Span;
}

/** The kind of route guard. */
export type RouteGuardKind =
  | "beforeEnter"
  | "beforeRouteEnter"
  | "beforeRouteLeave"
  | "onBeforeRouteLeave"
  | "onBeforeRouteUpdate"
  | "navigationGuard";

/** A navigation link found in a template. */
export interface NavigationLink {
  target: NavigationTarget;
  filePath: string;
  span: Span;
}

/** The target of a navigation link. */
export type NavigationTarget = { path: string } | { namedRoute: string } | "dynamic";

/** A `<RouterView>` or `<NuxtPage>` usage. */
export interface RouterViewLocation {
  filePath: string;
  span: Span;
  name?: string;
}

/** A layout definition (Nuxt `layouts/` directory). */
export interface LayoutDefinition {
  name: string;
  filePath: string;
  isDefault: boolean;
}

/** Complete route analysis result for a project. */
export interface RouteAnalysisSnapshot {
  framework: RoutingFramework;
  routes: RouteDefinition[];
  navigationLinks: NavigationLink[];
  layouts: LayoutDefinition[];
  routerViewLocations: RouterViewLocation[];
  globalGuards: RouteGuard[];
}

/** Result of route health analysis. */
export interface RouteHealthReport {
  missingComponents: RouteHealthIssue[];
  deadRoutes: RouteHealthIssue[];
  orphanViews: string[];
  missingLayouts: string[];
  duplicatePaths: string[];
  duplicateNames: string[];
  guardCoverage?: GuardCoverage;
}

/** A single route health issue. */
export interface RouteHealthIssue {
  routePath: string;
  detail: string;
}

/** Guard coverage statistics. */
export interface GuardCoverage {
  totalRoutes: number;
  routesWithGuards: number;
  hasGlobalGuard: boolean;
}

/** Span type (matches verter_span::Span). */
interface Span {
  start: number;
  end: number;
}
