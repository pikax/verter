import {
  TreeDataProvider,
  TreeItem,
  TreeItemCollapsibleState,
  EventEmitter,
  Event,
  window,
  workspace,
  ThemeIcon,
  ViewColumn,
  Disposable,
} from "vscode";
import { debounce } from "lodash";
import { LanguageClient } from "vscode-languageclient/node";
import { RequestType, type PatchClient } from "@verter/language-shared";
import { VirtualFileContentProvider } from "./VirtualFileManager";

export interface UnifiedVirtualFileItem {
  kind: string;
  lang: string;
  code: string;
  sourceMap: string | null;
  stale: boolean;
  isTsx: boolean;
  sourceUri: string;
}

export class UnifiedVirtualFilesProvider
  implements TreeDataProvider<UnifiedVirtualFileItem>, Disposable
{
  private _onDidChangeTreeData = new EventEmitter<UnifiedVirtualFileItem | undefined>();
  readonly onDidChangeTreeData: Event<UnifiedVirtualFileItem | undefined> =
    this._onDidChangeTreeData.event;

  private cachedItems: UnifiedVirtualFileItem[] = [];
  private subscriptions: Disposable[] = [];

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private contentProvider: VirtualFileContentProvider,
    private getLastVueUri: () => string | undefined,
  ) {
    this.subscriptions.push(
      window.onDidChangeActiveTextEditor((editor) => {
        if (editor?.document?.languageId === "vue") {
          this.refresh();
        }
      }),
    );

    this.subscriptions.push(
      workspace.onDidChangeTextDocument(
        debounce((e) => {
          if (e.document.languageId === "vue") {
            this.refresh();
          }
        }, 500),
      ),
    );
  }

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: UnifiedVirtualFileItem): TreeItem {
    const label = element.isTsx
      ? "IDE (TSX)"
      : element.kind === "api"
        ? "API (d.vue.ts)"
        : element.kind;
    const item = new TreeItem(label, TreeItemCollapsibleState.None);

    const parts: string[] = [];
    if (element.stale) parts.push("stale");
    parts.push(element.lang);
    if (element.sourceMap) parts.push("has source map");

    item.description = parts.join(" · ");
    item.iconPath = this.getIcon(element);
    item.tooltip = [
      `${element.kind} (${element.lang})`,
      element.stale ? "Stale — content may be outdated" : "",
      element.sourceMap ? "Source map available" : "No source map",
    ]
      .filter(Boolean)
      .join("\n");

    // Default click opens source map visualization with this file's tab pre-selected
    if (element.sourceMap) {
      item.command = {
        command: "verter.showSourceMapForFile",
        title: "Show Source Map",
        arguments: [element],
      };
    } else {
      item.command = {
        command: "verter.openVirtualFile",
        title: "Open Virtual File",
        arguments: [element],
      };
    }

    item.contextValue = "virtual-file";
    return item;
  }

  async getChildren(element?: UnifiedVirtualFileItem): Promise<UnifiedVirtualFileItem[]> {
    if (element) return [];

    const editor = window.activeTextEditor;
    const sourceUri =
      editor?.document?.languageId === "vue"
        ? editor.document.uri.toString()
        : this.getLastVueUri();

    if (!sourceUri) return [];

    try {
      const response = await this.getClient().sendRequest(RequestType.GetVirtualFiles, {
        uri: sourceUri,
      });

      if (!response) return [];

      const items: UnifiedVirtualFileItem[] = [];

      // Add IDE entry (TSX/JSX for template type checking)
      if (response.ide) {
        items.push({
          kind: "ide",
          lang: response.ide.isJs ? "jsx" : "tsx",
          code: response.ide.code,
          sourceMap: response.ide.sourceMap,
          stale: false,
          isTsx: !response.ide.isJs,
          sourceUri,
        });
      }

      // Add API entry (declaration output for cross-file type resolution)
      if (response.api) {
        items.push({
          kind: "api",
          lang: response.api.isJs ? "js" : "ts",
          code: response.api.code,
          sourceMap: response.api.sourceMap,
          stale: false,
          isTsx: false,
          sourceUri,
        });
      }

      // Add virtual file entries
      for (const vf of response.virtualFiles ?? []) {
        items.push({
          kind: vf.kind,
          lang: vf.lang,
          code: vf.code,
          sourceMap: vf.sourceMap,
          stale: vf.stale,
          isTsx: false,
          sourceUri,
        });
      }

      this.cachedItems = items;
      return items;
    } catch {
      return this.cachedItems;
    }
  }

  /**
   * Get all cached virtual file items (used by the source map webview panel).
   */
  getCachedItems(): UnifiedVirtualFileItem[] {
    return this.cachedItems;
  }

  private getIcon(item: UnifiedVirtualFileItem): ThemeIcon {
    if (item.isTsx) return new ThemeIcon("symbol-type-parameter");
    switch (item.kind) {
      case "api":
        return new ThemeIcon("symbol-interface");
      case "main":
        return new ThemeIcon("file-code");
      case "script":
        return new ThemeIcon("symbol-method");
      case "template":
        return new ThemeIcon("symbol-snippet");
      default:
        if (item.kind.startsWith("style")) return new ThemeIcon("paintcan");
        if (item.kind.startsWith("custom")) return new ThemeIcon("extensions");
        return new ThemeIcon("file");
    }
  }

  /**
   * Open a virtual file via the content provider (no disk writes).
   */
  async openVirtualFile(item: UnifiedVirtualFileItem): Promise<void> {
    const uri = this.contentProvider.setContent(item.kind, item.lang, item.sourceUri, item.code);

    const doc = await workspace.openTextDocument(uri);
    await window.showTextDocument(doc, {
      preview: true,
      preserveFocus: true,
      viewColumn: ViewColumn.Beside,
    });
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
  }
}
