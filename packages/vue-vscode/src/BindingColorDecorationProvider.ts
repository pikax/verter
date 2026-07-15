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
import type { AnalyzedBinding, AnalyzedMacro, FileAnalysisSnapshot } from "@verter/language-shared";
import { utf16OffsetToPosition } from "./utils";
import { isFrameworkCarrierLanguageId } from "./frameworkWiring";

/** Binding category for decoration coloring. */
type BindingCategory =
  | "ref"
  | "computed"
  | "reactive"
  | "prop"
  | "composable"
  | "mutable"
  | "function";

type DecorationStyle = "background" | "underline";
type DecorationScope = "template" | "all";

/** Theme color IDs (registered in package.json contributes.colors). */
const CATEGORY_THEME_COLORS: Record<BindingCategory, string> = {
  ref: "verter.binding.ref",
  computed: "verter.binding.computed",
  reactive: "verter.binding.reactive",
  prop: "verter.binding.prop",
  composable: "verter.binding.composable",
  mutable: "verter.binding.mutable",
  function: "verter.binding.function",
};

/** Underline colors per category (used when textDecoration can't reference ThemeColor). */
const CATEGORY_UNDERLINE_COLORS: Record<BindingCategory, { dark: string; light: string }> = {
  ref: { dark: "#4285f460", light: "#1a73e850" },
  computed: { dark: "#a142f460", light: "#7b1fa250" },
  reactive: { dark: "#00bcd460", light: "#00897b50" },
  prop: { dark: "#ff980060", light: "#e6510050" },
  composable: { dark: "#e91e6360", light: "#c2185b50" },
  mutable: { dark: "#ffc10760", light: "#f5960050" },
  function: { dark: "#4caf5060", light: "#2e7d3250" },
};

const ALL_CATEGORIES: BindingCategory[] = [
  "ref",
  "computed",
  "reactive",
  "prop",
  "composable",
  "mutable",
  "function",
];

/**
 * Classify a binding into a decoration category, or return undefined for no decoration.
 */
function classifyBinding(
  binding: AnalyzedBinding,
  propBindingNames: Set<string>,
): BindingCategory | undefined {
  // Props take priority (detected via macro cross-reference)
  if (propBindingNames.has(binding.name)) {
    return "prop";
  }

  // Direct defineProps assignment: const props = defineProps<...>()
  if (binding.initializer?.FunctionCall?.vueApi === "DefineProps") {
    return "prop";
  }

  // Reactivity-based classification
  switch (binding.reactivityKind) {
    case "Ref":
      return "ref";
    case "Computed":
      return "computed";
    case "Reactive":
      return "reactive";
    case "MaybeRef":
      return "composable";
    case "Mutable":
      return "mutable";
  }

  // Declaration-kind classification
  if (binding.kind === "Function" || binding.kind === "AsyncFunction") {
    return "function";
  }

  // Plain Const, Class, Var with no reactivity — no decoration
  return undefined;
}

/**
 * Build a set of binding names that are destructured from defineProps.
 *
 * Heuristic: if a DefineProps macro exists and a binding is a Const with no
 * initializer and isn't from an import, it's likely a destructured prop.
 */
function buildPropBindingNames(analysis: FileAnalysisSnapshot): Set<string> {
  const names = new Set<string>();

  const hasDefineProps = analysis.macros.some((m: AnalyzedMacro) => m.kind === "DefineProps");
  if (!hasDefineProps) return names;

  // Collect all imported binding names for exclusion
  const importedNames = new Set<string>();
  for (const imp of analysis.imports) {
    for (const b of imp.bindings) {
      importedNames.add(b.name);
    }
  }

  // Any Const binding with no initializer that isn't an import is likely a destructured prop
  for (const binding of analysis.bindings) {
    if (
      binding.kind === "Const" &&
      binding.reactivityKind === "None" &&
      !binding.initializer &&
      !importedNames.has(binding.name)
    ) {
      names.add(binding.name);
    }
  }

  // Also include the binding name of the DefineProps macro itself (e.g., `const props = ...`)
  for (const macro of analysis.macros) {
    if (macro.kind === "DefineProps" && macro.bindingName) {
      names.add(macro.bindingName);
    }
  }

  return names;
}

/**
 * Provides faint color-coded decorations for bindings based on their reactivity type.
 *
 * Categories: ref (blue), computed (purple), reactive (teal), prop (orange),
 * composable (pink), mutable (amber), function (green).
 *
 * Follows the same pattern as VueApiDecorationProvider.
 */
export class BindingColorDecorationProvider implements Disposable {
  private decorationTypes: Map<BindingCategory, TextEditorDecorationType> = new Map();
  private subscriptions: Disposable[] = [];
  private enabled = false;
  private scope: DecorationScope = "template";
  private style: DecorationStyle = "background";
  private _lastState: Map<
    string,
    Array<{ startLine: number; startChar: number; endLine: number; endChar: number }>
  > = new Map();

  constructor(private getClient: () => PatchClient<LanguageClient>) {
    this.readConfig();

    this.subscriptions.push(
      workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration("verter.decorations")) {
          const oldStyle = this.style;
          this.readConfig();
          // Recreate decoration types if style changed
          if (oldStyle !== this.style && this.enabled) {
            this.disposeDecorationTypes();
            this.ensureDecorationTypes();
          }
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
    this.enabled = config.get("bindingColors", true);
    this.scope = config.get<DecorationScope>("bindingColorsScope", "template");
    this.style = config.get<DecorationStyle>("bindingColorsStyle", "background");

    if (this.enabled) {
      this.ensureDecorationTypes();
    } else {
      this.clearAllDecorations();
      this.disposeDecorationTypes();
    }
  }

  private ensureDecorationTypes(): void {
    if (this.decorationTypes.size > 0) return;

    for (const category of ALL_CATEGORIES) {
      const decorationType =
        this.style === "underline"
          ? this.createUnderlineDecorationType(category)
          : this.createBackgroundDecorationType(category);
      this.decorationTypes.set(category, decorationType);
    }
  }

  private createBackgroundDecorationType(category: BindingCategory): TextEditorDecorationType {
    return window.createTextEditorDecorationType({
      rangeBehavior: DecorationRangeBehavior.ClosedClosed,
      backgroundColor: { id: CATEGORY_THEME_COLORS[category] },
      borderRadius: "2px",
    });
  }

  private createUnderlineDecorationType(category: BindingCategory): TextEditorDecorationType {
    const colors = CATEGORY_UNDERLINE_COLORS[category];
    return window.createTextEditorDecorationType({
      rangeBehavior: DecorationRangeBehavior.ClosedClosed,
      light: {
        textDecoration: `none; border-bottom: 1.5px dotted ${colors.light}`,
      },
      dark: {
        textDecoration: `none; border-bottom: 1.5px dotted ${colors.dark}`,
      },
    });
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
    if (!editor || !isFrameworkCarrierLanguageId(editor.document.languageId) || !this.enabled) {
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

    if (!analysis?.bindings?.length) {
      this.clearAllDecorations();
      return;
    }

    const sourceText = editor.document.getText();

    // Build prop detection set
    const propBindingNames = buildPropBindingNames(analysis);

    // Build binding name → category map
    const bindingCategoryMap = new Map<string, BindingCategory>();
    for (const binding of analysis.bindings) {
      const category = classifyBinding(binding, propBindingNames);
      if (category) {
        bindingCategoryMap.set(binding.name, category);
      }
    }

    // Initialize range groups
    const grouped = new Map<BindingCategory, Range[]>();
    for (const category of ALL_CATEGORIES) {
      grouped.set(category, []);
    }

    // Template binding occurrences
    if (analysis.template?.bindingOccurrences) {
      for (const occ of analysis.template.bindingOccurrences) {
        const category = bindingCategoryMap.get(occ.name);
        if (!category) continue;

        const start = utf16OffsetToPosition(sourceText, occ.spanStart);
        const end = utf16OffsetToPosition(sourceText, occ.spanEnd);
        grouped.get(category)!.push(new Range(start, end));
      }
    }

    // Script binding declarations (when scope is "all")
    if (this.scope === "all") {
      for (const binding of analysis.bindings) {
        const category = bindingCategoryMap.get(binding.name);
        if (!category) continue;

        const start = utf16OffsetToPosition(sourceText, binding.spanStart);
        const end = utf16OffsetToPosition(sourceText, binding.spanEnd);
        grouped.get(category)!.push(new Range(start, end));
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
