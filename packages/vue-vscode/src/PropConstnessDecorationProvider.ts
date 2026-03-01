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
import type {
  FileAnalysisSnapshot,
  TemplatePropUsage,
  PropValueConstness,
} from "@verter/language-shared";
import { utf16OffsetToPosition } from "./utils";

type ConstnessCategory = "const" | "dynamic";

/** Theme color IDs (registered in package.json contributes.colors). */
const CONSTNESS_THEME_COLORS: Record<ConstnessCategory, string> = {
  const: "verter.propConstness.const",
  dynamic: "verter.propConstness.dynamic",
};

/**
 * Provides faint background decorations on component prop attributes based on
 * their constness classification from cross-file analysis.
 *
 * Const props (always passed a literal or const binding) are highlighted in green.
 * Dynamic props (refs, computed, etc.) are highlighted in blue.
 *
 * This helps visualize which props are eligible for cross-file optimization:
 * props that are always const across all usage sites can skip reactive tracking.
 */
export class PropConstnessDecorationProvider implements Disposable {
  private decorationTypes: Map<ConstnessCategory, TextEditorDecorationType> =
    new Map();
  private subscriptions: Disposable[] = [];
  private enabled = false;

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
    this.enabled = config.get("propConstness", false);

    if (this.enabled) {
      this.ensureDecorationTypes();
    } else {
      this.clearAllDecorations();
      this.disposeDecorationTypes();
    }
  }

  private ensureDecorationTypes(): void {
    if (this.decorationTypes.size > 0) return;

    for (const category of ["const", "dynamic"] as ConstnessCategory[]) {
      const decorationType = window.createTextEditorDecorationType({
        rangeBehavior: DecorationRangeBehavior.ClosedClosed,
        backgroundColor: { id: CONSTNESS_THEME_COLORS[category] },
        borderRadius: "2px",
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

    let analysis: FileAnalysisSnapshot | null;
    try {
      analysis = await this.getClient().sendRequest(RequestType.GetAnalysis, {
        uri: sourceUri,
      });
    } catch {
      return;
    }

    if (!analysis?.template?.components?.length) {
      this.clearAllDecorations();
      return;
    }

    const sourceText = editor.document.getText();

    const grouped = new Map<ConstnessCategory, Range[]>();
    grouped.set("const", []);
    grouped.set("dynamic", []);

    for (const comp of analysis.template.components) {
      for (const prop of comp.props) {
        const category = classifyPropConstness(prop.constness);
        if (!category) continue;

        // Only decorate props that have valid spans
        if (prop.spanStart === 0 && prop.spanEnd === 0) continue;

        const start = utf16OffsetToPosition(sourceText, prop.spanStart);
        const end = utf16OffsetToPosition(sourceText, prop.spanEnd);
        grouped.get(category)!.push(new Range(start, end));
      }
    }

    for (const [category, dt] of this.decorationTypes) {
      const ranges = grouped.get(category) ?? [];
      editor.setDecorations(dt, ranges);
    }
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
    this.disposeDecorationTypes();
  }
}

function classifyPropConstness(
  constness: PropValueConstness,
): ConstnessCategory | undefined {
  switch (constness) {
    case "Const":
      return "const";
    case "Dynamic":
      return "dynamic";
    default:
      return undefined; // Unknown — no decoration
  }
}
