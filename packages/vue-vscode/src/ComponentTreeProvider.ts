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
  ComponentParentInfo,
  FileAnalysisSnapshot,
  TemplateComponentUsage,
  TemplatePropUsage,
} from "@verter/language-shared";
import { basename, dirname, resolve } from "path";
import { isCarrierComponentImport, isFrameworkCarrierLanguageId } from "./frameworkWiring";

// ── Node types for the component tree ──────────────────────────

export type ComponentTreeItem = SectionNode | ComponentNode | ParentFileNode | PropNode | SlotNode;

export interface SectionNode {
  type: "section";
  section: "parents" | "children";
  sourceFileUri: string;
}

export interface ComponentNode {
  type: "component";
  component: TemplateComponentUsage;
  sourceFileUri: string;
}

export interface ParentFileNode {
  type: "parent-file";
  parent: ComponentParentInfo;
  sourceFileUri: string;
}

export interface PropNode {
  type: "prop";
  prop: TemplatePropUsage;
  parentComponent: string;
}

export interface SlotNode {
  type: "slot";
  slotName: string;
  parentComponent: string;
}

// ── Tree data provider ─────────────────────────────────────────

export class ComponentTreeProvider implements TreeDataProvider<ComponentTreeItem>, Disposable {
  private _onDidChangeTreeData = new EventEmitter<ComponentTreeItem | undefined>();
  readonly onDidChangeTreeData: Event<ComponentTreeItem | undefined> =
    this._onDidChangeTreeData.event;

  private cachedAnalysis: FileAnalysisSnapshot | null = null;
  private cachedBindingTypes: Record<string, string | null> = {};
  private cachedParents: ComponentParentInfo[] = [];
  private cachedSourceUri: string | undefined;
  private subscriptions: Disposable[] = [];

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private getLastVueUri: () => string | undefined,
  ) {
    this.subscriptions.push(
      window.onDidChangeActiveTextEditor((editor) => {
        if (isFrameworkCarrierLanguageId(editor?.document?.languageId)) {
          this.refresh();
        }
      }),
    );

    this.subscriptions.push(
      workspace.onDidChangeTextDocument(
        debounce((e) => {
          if (isFrameworkCarrierLanguageId(e.document.languageId)) {
            this.refresh();
          }
        }, 500),
      ),
    );
  }

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: ComponentTreeItem): TreeItem {
    switch (element.type) {
      case "section":
        return this.getSectionTreeItem(element);
      case "component":
        return this.getComponentTreeItem(element);
      case "parent-file":
        return this.getParentFileTreeItem(element);
      case "prop":
        return this.getPropTreeItem(element);
      case "slot":
        return this.getSlotTreeItem(element);
    }
  }

  async getChildren(element?: ComponentTreeItem): Promise<ComponentTreeItem[]> {
    // Root: fetch analysis and return two section nodes
    if (!element) {
      return this.getRootChildren();
    }

    // Section nodes expand to show parents or children
    if (element.type === "section") {
      if (element.section === "children") {
        return this.getChildrenComponents(element.sourceFileUri);
      }
      return this.getParentFiles(element.sourceFileUri);
    }

    // Component nodes expand to show props + slots
    if (element.type === "component") {
      return this.getComponentDetails(element);
    }

    // Parent file nodes expand to show props + slots they pass
    if (element.type === "parent-file") {
      return this.getParentFileDetails(element);
    }

    return [];
  }

  // ── Tree item renderers ──────────────────────────────────────

  private getSectionTreeItem(element: SectionNode): TreeItem {
    if (element.section === "parents") {
      const count = this.cachedParents.length;
      const item = new TreeItem(
        "Parents",
        count > 0 ? TreeItemCollapsibleState.Collapsed : TreeItemCollapsibleState.None,
      );
      item.description =
        count > 0
          ? `${count} file${count !== 1 ? "s" : ""} use this component`
          : "no files use this component";
      item.iconPath = new ThemeIcon("references");
      item.tooltip = "Files that use this component in their template";
      item.contextValue = "section-parents";
      return item;
    }

    // Children section
    const childCount = this.cachedAnalysis?.template?.components?.length ?? 0;
    const item = new TreeItem(
      "Children",
      childCount > 0 ? TreeItemCollapsibleState.Expanded : TreeItemCollapsibleState.None,
    );
    item.description =
      childCount > 0
        ? `${childCount} component${childCount !== 1 ? "s" : ""} used`
        : "no components used";
    item.iconPath = new ThemeIcon("symbol-class");
    item.tooltip = "Components used in this file's template";
    item.contextValue = "section-children";
    return item;
  }

  private getComponentTreeItem(element: ComponentNode): TreeItem {
    const comp = element.component;
    const label = comp.isDynamic ? `<component :is="..."> (dynamic)` : comp.name;
    const item = new TreeItem(label, TreeItemCollapsibleState.Collapsed);

    item.description = comp.importSource ? `from "${comp.importSource}"` : "(global)";
    item.iconPath = new ThemeIcon("symbol-class");
    item.tooltip = [
      `Component: ${comp.name}`,
      comp.importSource ? `Import: ${comp.importSource}` : "Global component",
      comp.isDynamic ? "Dynamic component" : "",
      comp.hasSpread ? "Has v-bind spread" : "",
      `Props: ${(comp.props ?? []).length}`,
      `Slots: ${(comp.slotsUsed ?? []).join(", ") || "none"}`,
    ]
      .filter(Boolean)
      .join("\n");

    // Click opens the defining file
    if (isCarrierComponentImport(comp.importSource)) {
      item.command = {
        command: "verter.goToComponent",
        title: "Go to Component",
        arguments: [element],
      };
    }

    item.contextValue = "component";
    return item;
  }

  private getParentFileTreeItem(element: ParentFileNode): TreeItem {
    const parent = element.parent;
    const fileName = this.extractFileName(parent.filePath);
    const propCount = parent.props.length;
    const slotCount = parent.slotsUsed.length;

    const item = new TreeItem(
      fileName,
      propCount + slotCount > 0
        ? TreeItemCollapsibleState.Collapsed
        : TreeItemCollapsibleState.None,
    );

    const detailParts: string[] = [];
    if (propCount > 0) detailParts.push(`${propCount} prop${propCount !== 1 ? "s" : ""}`);
    if (slotCount > 0) detailParts.push(`${slotCount} slot${slotCount !== 1 ? "s" : ""}`);
    item.description =
      detailParts.length > 0
        ? `as <${parent.componentName}> (${detailParts.join(", ")})`
        : `as <${parent.componentName}>`;

    item.iconPath = new ThemeIcon("file-code");
    item.tooltip = [
      `Parent: ${parent.filePath}`,
      `Uses as: <${parent.componentName}>`,
      `Props: ${propCount}`,
      `Slots: ${slotCount}`,
    ].join("\n");

    // Click navigates to the parent file
    item.command = {
      command: "verter.goToParentFile",
      title: "Go to Parent File",
      arguments: [element],
    };

    item.contextValue = "parent-file";
    return item;
  }

  private getPropTreeItem(element: PropNode): TreeItem {
    const prop = element.prop;
    const constness =
      prop.constness === "Const" ? "const" : prop.constness === "Dynamic" ? "dynamic" : "";

    // Resolve type from referenced bindings if available
    const resolvedTypes: string[] = [];
    for (const bindingName of prop.referencedBindings ?? []) {
      const t = this.cachedBindingTypes[bindingName];
      if (t) resolvedTypes.push(t);
    }
    const typeStr = resolvedTypes.length > 0 ? resolvedTypes[0] : null;

    const descParts = [typeStr, constness].filter(Boolean);
    const item = new TreeItem(`prop: ${prop.name}`, TreeItemCollapsibleState.None);
    item.description = descParts.length > 0 ? `(${descParts.join(", ")})` : "";
    item.iconPath = new ThemeIcon("symbol-property");
    item.tooltip = [
      `Prop: ${prop.name}`,
      `Bound: ${prop.isBound}`,
      `Constness: ${prop.constness}`,
      typeStr ? `Type: ${typeStr}` : "",
      prop.referencedBindings?.length ? `Bindings: ${prop.referencedBindings.join(", ")}` : "",
      prop.fromSpread ? "From v-bind spread" : "",
    ]
      .filter(Boolean)
      .join("\n");
    return item;
  }

  private getSlotTreeItem(element: SlotNode): TreeItem {
    const item = new TreeItem(`slot: ${element.slotName}`, TreeItemCollapsibleState.None);
    item.iconPath = new ThemeIcon("symbol-interface");
    return item;
  }

  // ── Data fetching ────────────────────────────────────────────

  private async getRootChildren(): Promise<ComponentTreeItem[]> {
    const editor = window.activeTextEditor;
    const sourceUri = isFrameworkCarrierLanguageId(editor?.document?.languageId)
      ? editor!.document.uri.toString()
      : this.getLastVueUri();

    if (!sourceUri) return [];

    try {
      const [analysis, bindingTypes, parentsResponse] = await Promise.all([
        this.getClient().sendRequest(RequestType.GetAnalysis, { uri: sourceUri }),
        this.getClient()
          .sendRequest(RequestType.GetBindingTypes, { uri: sourceUri })
          .catch(() => null),
        this.getClient()
          .sendRequest(RequestType.GetComponentParents, { uri: sourceUri })
          .catch(() => null),
      ]);

      if (!analysis) return [];
      this.cachedAnalysis = analysis;
      this.cachedBindingTypes = bindingTypes ?? {};
      this.cachedParents = parentsResponse?.parents ?? [];
      this.cachedSourceUri = sourceUri;

      // Return two section nodes: Parents and Children
      return [
        {
          type: "section" as const,
          section: "parents" as const,
          sourceFileUri: sourceUri,
        },
        {
          type: "section" as const,
          section: "children" as const,
          sourceFileUri: sourceUri,
        },
      ];
    } catch {
      return [];
    }
  }

  private getChildrenComponents(sourceFileUri: string): ComponentTreeItem[] {
    const components = this.cachedAnalysis?.template?.components ?? [];
    return components.map(
      (comp): ComponentNode => ({
        type: "component",
        component: comp,
        sourceFileUri,
      }),
    );
  }

  private getParentFiles(sourceFileUri: string): ComponentTreeItem[] {
    return this.cachedParents.map(
      (parent): ParentFileNode => ({
        type: "parent-file",
        parent,
        sourceFileUri,
      }),
    );
  }

  private getComponentDetails(element: ComponentNode): ComponentTreeItem[] {
    const children: ComponentTreeItem[] = [];

    for (const prop of element.component.props ?? []) {
      children.push({
        type: "prop",
        prop,
        parentComponent: element.component.name,
      });
    }

    for (const slot of element.component.slotsUsed ?? []) {
      children.push({
        type: "slot",
        slotName: slot,
        parentComponent: element.component.name,
      });
    }

    return children;
  }

  private getParentFileDetails(element: ParentFileNode): ComponentTreeItem[] {
    const children: ComponentTreeItem[] = [];
    const parent = element.parent;

    for (const propJson of parent.props) {
      // Props come as serialized JSON objects from the LSP
      const prop = propJson as unknown as TemplatePropUsage;
      children.push({
        type: "prop",
        prop,
        parentComponent: parent.componentName,
      });
    }

    for (const slot of parent.slotsUsed) {
      children.push({
        type: "slot",
        slotName: slot,
        parentComponent: parent.componentName,
      });
    }

    return children;
  }

  // ── Helpers ──────────────────────────────────────────────────

  private extractFileName(filePath: string): string {
    // Handle both URI strings and file paths
    const normalized = filePath.replace(/\\/g, "/");
    const lastSlash = normalized.lastIndexOf("/");
    return lastSlash >= 0 ? normalized.slice(lastSlash + 1) : normalized;
  }

  /**
   * Navigate to a component's defining file.
   */
  async goToComponent(node: ComponentNode): Promise<void> {
    const comp = node.component;
    if (!isCarrierComponentImport(comp.importSource)) return;

    const sourceUri = node.sourceFileUri || this.cachedSourceUri;
    if (!sourceUri) return;

    try {
      const parsed = Uri.parse(sourceUri);
      const currentDir = dirname(parsed.fsPath);
      const targetPath = resolve(currentDir, comp.importSource);
      const doc = await workspace.openTextDocument(Uri.file(targetPath));
      await window.showTextDocument(doc);
    } catch {
      // File might not exist
    }
  }

  /**
   * Navigate to a parent file.
   */
  async goToParentFile(node: ParentFileNode): Promise<void> {
    const filePath = node.parent.filePath;
    try {
      const doc = await workspace.openTextDocument(Uri.file(filePath));
      await window.showTextDocument(doc);
    } catch {
      // File might not exist
    }
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
  }
}
