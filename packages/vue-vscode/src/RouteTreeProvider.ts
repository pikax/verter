import {
  TreeDataProvider,
  TreeItem,
  TreeItemCollapsibleState,
  EventEmitter,
  Event,
  window,
  workspace,
  Uri,
  ThemeIcon,
  Disposable,
} from "vscode";
import { debounce } from "lodash";
import { LanguageClient } from "vscode-languageclient/node";
import { RequestType, type PatchClient } from "@verter/language-shared";
import type {
  RouteAnalysisSnapshot,
  RouteDefinition,
  LayoutDefinition,
  NavigationLink,
  RouterViewLocation,
  RouteGuard,
} from "@verter/language-shared";

// ── Node types for the route tree ──────────────────────────────

export type RouteTreeItem =
  | FrameworkNode
  | SectionNode
  | RouteNode
  | ComponentNode
  | GuardNode
  | LayoutNode
  | NavLinkNode
  | RouterViewNode;

export interface FrameworkNode {
  type: "framework";
  label: string;
}

export interface SectionNode {
  type: "section";
  section: "routes" | "layouts" | "routerViews" | "navLinks";
}

export interface RouteNode {
  type: "route";
  route: RouteDefinition;
}

export interface ComponentNode {
  type: "component";
  filePath: string;
}

export interface GuardNode {
  type: "guard";
  guard: RouteGuard;
}

export interface LayoutNode {
  type: "layout";
  layout: LayoutDefinition;
}

export interface NavLinkNode {
  type: "navLink";
  link: NavigationLink;
}

export interface RouterViewNode {
  type: "routerView";
  view: RouterViewLocation;
}

// ── Tree data provider ─────────────────────────────────────────

const FRAMEWORK_LABELS: Record<string, string> = {
  vueRouter: "Vue Router",
  nuxtPages: "Nuxt Pages",
  unpluginVueRouter: "unplugin-vue-router",
  unknown: "Unknown",
};

export class RouteTreeProvider implements TreeDataProvider<RouteTreeItem>, Disposable {
  private _onDidChangeTreeData = new EventEmitter<RouteTreeItem | undefined>();
  readonly onDidChangeTreeData: Event<RouteTreeItem | undefined> = this._onDidChangeTreeData.event;

  private cachedSnapshot: RouteAnalysisSnapshot | null = null;
  private subscriptions: Disposable[] = [];

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private getLastVueUri: () => string | undefined,
  ) {
    // Refresh on active editor change (any file type — router configs are .ts/.js)
    this.subscriptions.push(
      window.onDidChangeActiveTextEditor(() => {
        this.refresh();
      }),
    );

    // Refresh on any text document change (debounced)
    this.subscriptions.push(
      workspace.onDidChangeTextDocument(
        debounce(() => {
          this.refresh();
        }, 500),
      ),
    );
  }

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: RouteTreeItem): TreeItem {
    switch (element.type) {
      case "framework":
        return this.getFrameworkTreeItem(element);
      case "section":
        return this.getSectionTreeItem(element);
      case "route":
        return this.getRouteTreeItem(element);
      case "component":
        return this.getComponentTreeItem(element);
      case "guard":
        return this.getGuardTreeItem(element);
      case "layout":
        return this.getLayoutTreeItem(element);
      case "navLink":
        return this.getNavLinkTreeItem(element);
      case "routerView":
        return this.getRouterViewTreeItem(element);
    }
  }

  async getChildren(element?: RouteTreeItem): Promise<RouteTreeItem[]> {
    if (!element) {
      return this.getRootChildren();
    }

    switch (element.type) {
      case "framework":
        return this.getFrameworkChildren();
      case "section":
        return this.getSectionChildren(element);
      case "route":
        return this.getRouteChildren(element);
      default:
        return [];
    }
  }

  // ── Tree item renderers ──────────────────────────────────────

  private getFrameworkTreeItem(element: FrameworkNode): TreeItem {
    const item = new TreeItem(element.label, TreeItemCollapsibleState.Expanded);
    item.iconPath = new ThemeIcon("symbol-namespace");
    item.contextValue = "framework";
    return item;
  }

  private getSectionTreeItem(element: SectionNode): TreeItem {
    const snapshot = this.cachedSnapshot;
    switch (element.section) {
      case "routes": {
        const count = snapshot?.routes.length ?? 0;
        const item = new TreeItem(
          "Routes",
          count > 0 ? TreeItemCollapsibleState.Expanded : TreeItemCollapsibleState.None,
        );
        item.description = `${count} top-level`;
        item.iconPath = new ThemeIcon("symbol-event");
        return item;
      }
      case "layouts": {
        const count = snapshot?.layouts.length ?? 0;
        const item = new TreeItem(
          "Layouts",
          count > 0 ? TreeItemCollapsibleState.Collapsed : TreeItemCollapsibleState.None,
        );
        item.description = `${count}`;
        item.iconPath = new ThemeIcon("layout");
        return item;
      }
      case "routerViews": {
        const count = snapshot?.routerViewLocations.length ?? 0;
        const item = new TreeItem(
          "Router Views",
          count > 0 ? TreeItemCollapsibleState.Collapsed : TreeItemCollapsibleState.None,
        );
        item.description = `${count}`;
        item.iconPath = new ThemeIcon("window");
        return item;
      }
      case "navLinks": {
        const count = snapshot?.navigationLinks.length ?? 0;
        const item = new TreeItem(
          "Navigation Links",
          count > 0 ? TreeItemCollapsibleState.Collapsed : TreeItemCollapsibleState.None,
        );
        item.description = `${count}`;
        item.iconPath = new ThemeIcon("link");
        return item;
      }
    }
  }

  private getRouteTreeItem(element: RouteNode): TreeItem {
    const route = element.route;
    const hasChildren = route.children.length > 0 || route.guards.length > 0;
    const item = new TreeItem(
      route.path || "/",
      hasChildren ? TreeItemCollapsibleState.Collapsed : TreeItemCollapsibleState.None,
    );

    const descParts: string[] = [];
    if (route.name) descParts.push(route.name);
    if (route.componentPath) descParts.push(extractFileName(route.componentPath));
    item.description = descParts.join(" — ");

    item.iconPath = new ThemeIcon("symbol-event");
    item.tooltip = [
      `Path: ${route.fullPath}`,
      route.name ? `Name: ${route.name}` : "",
      route.componentPath ? `Component: ${route.componentPath}` : "",
      route.isLazy ? "Lazy loaded" : "",
      route.redirect ? `Redirect: ${route.redirect}` : "",
      route.guards.length > 0 ? `Guards: ${route.guards.length}` : "",
      route.children.length > 0 ? `Children: ${route.children.length}` : "",
    ]
      .filter(Boolean)
      .join("\n");

    // Click opens the component file
    if (route.componentPath) {
      item.command = {
        command: "verter.openRouteComponent",
        title: "Open Component",
        arguments: [route.componentPath],
      };
    }

    item.contextValue = "route";
    return item;
  }

  private getComponentTreeItem(element: ComponentNode): TreeItem {
    const item = new TreeItem(extractFileName(element.filePath), TreeItemCollapsibleState.None);
    item.description = element.filePath;
    item.iconPath = new ThemeIcon("file-code");
    item.command = {
      command: "verter.openRouteComponent",
      title: "Open File",
      arguments: [element.filePath],
    };
    return item;
  }

  private getGuardTreeItem(element: GuardNode): TreeItem {
    const guard = element.guard;
    const item = new TreeItem(guard.kind, TreeItemCollapsibleState.None);
    item.description = extractFileName(guard.filePath);
    item.iconPath = new ThemeIcon("shield");
    return item;
  }

  private getLayoutTreeItem(element: LayoutNode): TreeItem {
    const layout = element.layout;
    const label = layout.isDefault ? `${layout.name} (default)` : layout.name;
    const item = new TreeItem(label, TreeItemCollapsibleState.None);
    item.description = extractFileName(layout.filePath);
    item.iconPath = new ThemeIcon("layout");
    item.command = {
      command: "verter.openRouteComponent",
      title: "Open Layout",
      arguments: [layout.filePath],
    };
    return item;
  }

  private getNavLinkTreeItem(element: NavLinkNode): TreeItem {
    const link = element.link;
    const target = link.target;
    let label: string;
    if (typeof target === "string") {
      label = "(dynamic)";
    } else if ("path" in target) {
      label = target.path;
    } else {
      label = `name: ${target.namedRoute}`;
    }
    const item = new TreeItem(label, TreeItemCollapsibleState.None);
    item.description = extractFileName(link.filePath);
    item.iconPath = new ThemeIcon("link");
    return item;
  }

  private getRouterViewTreeItem(element: RouterViewNode): TreeItem {
    const view = element.view;
    const label = view.name ? `<RouterView name="${view.name}">` : "<RouterView>";
    const item = new TreeItem(label, TreeItemCollapsibleState.None);
    item.description = extractFileName(view.filePath);
    item.iconPath = new ThemeIcon("window");
    return item;
  }

  // ── Data fetching ────────────────────────────────────────────

  private async getRootChildren(): Promise<RouteTreeItem[]> {
    try {
      const snapshot = await this.getClient().sendRequest(
        RequestType.GetRouteTree,
        {} as Record<string, never>,
      );

      if (!snapshot) return [];
      this.cachedSnapshot = snapshot;

      const frameworkLabel = FRAMEWORK_LABELS[snapshot.framework] ?? snapshot.framework;

      return [{ type: "framework", label: frameworkLabel }];
    } catch {
      return [];
    }
  }

  private getFrameworkChildren(): RouteTreeItem[] {
    const snapshot = this.cachedSnapshot;
    if (!snapshot) return [];

    const sections: RouteTreeItem[] = [{ type: "section", section: "routes" }];

    if (snapshot.layouts.length > 0) {
      sections.push({ type: "section", section: "layouts" });
    }
    if (snapshot.routerViewLocations.length > 0) {
      sections.push({ type: "section", section: "routerViews" });
    }
    if (snapshot.navigationLinks.length > 0) {
      sections.push({ type: "section", section: "navLinks" });
    }

    return sections;
  }

  private getSectionChildren(element: SectionNode): RouteTreeItem[] {
    const snapshot = this.cachedSnapshot;
    if (!snapshot) return [];

    switch (element.section) {
      case "routes":
        return snapshot.routes.map((route): RouteNode => ({ type: "route", route }));
      case "layouts":
        return snapshot.layouts.map((layout): LayoutNode => ({ type: "layout", layout }));
      case "routerViews":
        return snapshot.routerViewLocations.map(
          (view): RouterViewNode => ({ type: "routerView", view }),
        );
      case "navLinks":
        return snapshot.navigationLinks.map((link): NavLinkNode => ({ type: "navLink", link }));
    }
  }

  private getRouteChildren(element: RouteNode): RouteTreeItem[] {
    const children: RouteTreeItem[] = [];
    const route = element.route;

    // Per-route guards
    for (const guard of route.guards) {
      children.push({ type: "guard", guard });
    }

    // Nested child routes
    for (const child of route.children) {
      children.push({ type: "route", route: child });
    }

    return children;
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
  }
}

function extractFileName(filePath: string): string {
  const normalized = filePath.replace(/\\/g, "/");
  const lastSlash = normalized.lastIndexOf("/");
  return lastSlash >= 0 ? normalized.slice(lastSlash + 1) : normalized;
}
