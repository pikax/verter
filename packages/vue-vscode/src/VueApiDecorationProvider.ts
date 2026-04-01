import {
  window,
  workspace,
  TextEditor,
  TextEditorDecorationType,
  Range,
  Disposable,
  DecorationRangeBehavior,
} from "vscode";
import { debounce } from "lodash";
import { LanguageClient } from "vscode-languageclient/node";
import { RequestType, type PatchClient } from "@verter/language-shared";
import type { VueApiCallSite } from "@verter/language-shared";
import { utf16OffsetToPosition } from "./utils";

/** Category of a Vue API call, used to group decorations by color. */
type VueApiCategory = "lifecycle" | "watcher" | "reactivity" | "dependency-injection" | "other";

/** Map Vue API names to decoration categories. */
function categorize(api: string): VueApiCategory {
  if (api.startsWith("On") || api === "onMounted" || api === "onUnmounted") {
    return "lifecycle";
  }
  switch (api) {
    // Lifecycle hooks use PascalCase enum names in the semantic snapshot.
    case "OnMounted":
    case "OnUnmounted":
    case "OnBeforeMount":
    case "OnBeforeUnmount":
    case "OnUpdated":
    case "OnBeforeUpdate":
    case "OnActivated":
    case "OnDeactivated":
    case "OnErrorCaptured":
    case "OnRenderTracked":
    case "OnRenderTriggered":
    case "OnServerPrefetch":
      return "lifecycle";

    // Watchers
    case "Watch":
    case "WatchEffect":
    case "WatchPostEffect":
    case "WatchSyncEffect":
      return "watcher";

    // Reactivity
    case "Ref":
    case "Computed":
    case "Reactive":
    case "ShallowRef":
    case "ShallowReactive":
    case "Readonly":
    case "ShallowReadonly":
    case "ToRef":
    case "ToRefs":
      return "reactivity";

    // Dependency injection
    case "Provide":
    case "Inject":
      return "dependency-injection";

    default:
      return "other";
  }
}

/** Theme colors for each category (uses ThemeColor references for theme compatibility). */
const CATEGORY_COLORS: Record<VueApiCategory, string> = {
  lifecycle: "charts.green",
  watcher: "charts.yellow",
  reactivity: "charts.blue",
  "dependency-injection": "charts.purple",
  other: "charts.foreground",
};

/** After-text annotations for categories. */
const CATEGORY_LABELS: Record<VueApiCategory, string> = {
  lifecycle: "lifecycle",
  watcher: "watcher",
  reactivity: "reactivity",
  "dependency-injection": "provide/inject",
  other: "vue api",
};

/**
 * Provides semantic decorations for Vue API call sites in .vue files.
 *
 * Fetches analysis data from the LSP and applies subtle inline decorations
 * to lifecycle hooks, watchers, reactivity primitives, and provide/inject calls.
 */
export class VueApiDecorationProvider implements Disposable {
  private decorationTypes: Map<VueApiCategory, TextEditorDecorationType> = new Map();
  private subscriptions: Disposable[] = [];
  private enabled = false;
  private _lastState: Map<
    string,
    Array<{ startLine: number; startChar: number; endLine: number; endChar: number }>
  > = new Map();

  constructor(private getClient: () => PatchClient<LanguageClient>) {
    this.readConfig();

    this.subscriptions.push(
      workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration("verter.decorations")) {
          this.readConfig();
          this.updateActiveEditor();
        }
      }),
    );

    this.subscriptions.push(
      window.onDidChangeActiveTextEditor(() => {
        this.updateActiveEditor();
      }),
    );

    this.subscriptions.push(
      workspace.onDidChangeTextDocument(
        debounce((e) => {
          const editor = window.activeTextEditor;
          if (editor && e.document === editor.document) {
            this.updateActiveEditor();
          }
        }, 500),
      ),
    );

    // Initial update
    this.updateActiveEditor();
  }

  private readConfig(): void {
    const config = workspace.getConfiguration("verter.decorations");
    this.enabled = config.get("vueApiCalls", true);

    if (this.enabled) {
      this.ensureDecorationTypes();
    } else {
      this.clearAllDecorations();
      this.disposeDecorationTypes();
    }
  }

  private ensureDecorationTypes(): void {
    if (this.decorationTypes.size > 0) return;

    for (const category of Object.keys(CATEGORY_COLORS) as VueApiCategory[]) {
      const themeColor = CATEGORY_COLORS[category];
      const decorationType = window.createTextEditorDecorationType({
        rangeBehavior: DecorationRangeBehavior.ClosedClosed,
        after: {
          contentText: ` // ${CATEGORY_LABELS[category]}`,
          color: { id: themeColor },
          fontStyle: "italic",
          fontWeight: "normal",
        },
      });
      this.decorationTypes.set(category, decorationType);
    }
  }

  private disposeDecorationTypes(): void {
    for (const dt of this.decorationTypes.values()) {
      dt.dispose();
    }
    this.decorationTypes.clear();
  }

  private clearAllDecorations(): void {
    const editor = window.activeTextEditor;
    if (!editor) return;
    for (const dt of this.decorationTypes.values()) {
      editor.setDecorations(dt, []);
    }
  }

  private async updateActiveEditor(): Promise<void> {
    const editor = window.activeTextEditor;
    if (!editor || editor.document.languageId !== "vue" || !this.enabled) {
      this.clearAllDecorations();
      return;
    }

    await this.applyDecorations(editor);
  }

  private async applyDecorations(editor: TextEditor): Promise<void> {
    const sourceUri = editor.document.uri.toString();

    let analysis;
    try {
      analysis = await this.getClient().sendRequest(RequestType.GetAnalysis, { uri: sourceUri });
    } catch {
      return;
    }

    if (!analysis?.vueApiCalls?.length) {
      this.clearAllDecorations();
      return;
    }

    const sourceText = editor.document.getText();

    // Group call sites by category
    const grouped = new Map<VueApiCategory, Range[]>();
    for (const category of this.decorationTypes.keys()) {
      grouped.set(category, []);
    }

    for (const call of analysis.vueApiCalls) {
      const category = categorize(call.api);
      const start = utf16OffsetToPosition(sourceText, call.spanStart);
      const end = utf16OffsetToPosition(sourceText, call.spanEnd);

      // Place the annotation at the end of the line containing the call
      const lineEnd = editor.document.lineAt(start.line).range.end;
      const ranges = grouped.get(category);
      if (ranges) {
        ranges.push(new Range(lineEnd, lineEnd));
      }
    }

    // Apply decorations per category
    this._lastState.clear();
    for (const [category, dt] of this.decorationTypes) {
      const ranges = grouped.get(category) ?? [];
      editor.setDecorations(dt, ranges);
      this._lastState.set(
        category,
        ranges.map((r) => ({
          startLine: r.start.line,
          startChar: r.start.character,
          endLine: r.end.line,
          endChar: r.end.character,
        })),
      );
    }
  }

  /** Returns the last-applied decoration ranges by category (for E2E testing). */
  getState(): Record<
    string,
    Array<{ startLine: number; startChar: number; endLine: number; endChar: number }>
  > {
    const result: Record<
      string,
      Array<{ startLine: number; startChar: number; endLine: number; endChar: number }>
    > = {};
    for (const [category, ranges] of this._lastState) {
      result[category] = ranges;
    }
    return result;
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
    this.disposeDecorationTypes();
  }
}
