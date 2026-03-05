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
  Position,
  Range,
  Disposable,
} from "vscode";
import { debounce } from "lodash";
import { LanguageClient } from "vscode-languageclient/node";
import { RequestType, type PatchClient } from "@verter/language-shared";
import type {
  FileAnalysisSnapshot,
  ProjectOverview,
} from "@verter/language-shared";
import { basename } from "path";
import { utf16OffsetToPosition } from "./utils";

export type AnalysisItem = CategoryItem | LeafItem;

export interface CategoryItem {
  type: "category";
  label: string;
  children: LeafItem[];
}

export interface LeafItem {
  type: "leaf";
  label: string;
  description: string;
  tooltip: string;
  icon: ThemeIcon;
  startPosition?: Position;
  endPosition?: Position;
  sourceUri?: string;
}

/** Vue lifecycle hooks and composables that indicate Vue API usage */
const VUE_LIFECYCLE_HOOKS = new Set([
  "onMounted",
  "onUnmounted",
  "onBeforeMount",
  "onBeforeUnmount",
  "onUpdated",
  "onBeforeUpdate",
  "onActivated",
  "onDeactivated",
  "onErrorCaptured",
  "onRenderTracked",
  "onRenderTriggered",
  "onServerPrefetch",
]);

const VUE_COMPOSABLES = new Set([
  "ref",
  "reactive",
  "computed",
  "watch",
  "watchEffect",
  "watchPostEffect",
  "watchSyncEffect",
  "provide",
  "inject",
  "toRef",
  "toRefs",
  "shallowRef",
  "shallowReactive",
  "readonly",
  "shallowReadonly",
  "useSlots",
  "useAttrs",
]);

interface ParsedImport {
  bindings: string[];
  source: string;
  line: number;
}

export class AnalysisTreeProvider
  implements TreeDataProvider<AnalysisItem>, Disposable
{
  private _onDidChangeTreeData = new EventEmitter<AnalysisItem | undefined>();
  readonly onDidChangeTreeData: Event<AnalysisItem | undefined> =
    this._onDidChangeTreeData.event;

  private cachedCategories: CategoryItem[] = [];
  private cachedProjectOverview: CategoryItem[] = [];
  private subscriptions: Disposable[] = [];

  constructor(
    private getClient: () => PatchClient<LanguageClient>,
    private getLastVueUri: () => string | undefined,
  ) {
    // Refresh on any editor change (not just .vue — we show for all file types)
    this.subscriptions.push(
      window.onDidChangeActiveTextEditor(() => {
        this.refresh();
      }),
    );

    this.subscriptions.push(
      workspace.onDidChangeTextDocument(
        debounce((e) => {
          if (
            e.document.languageId === "vue" ||
            e.document.languageId === "typescript" ||
            e.document.languageId === "javascript"
          ) {
            this.refresh();
          }
        }, 500),
      ),
    );
  }

  refresh(): void {
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(element: AnalysisItem): TreeItem {
    if (element.type === "category") {
      const item = new TreeItem(
        element.label,
        TreeItemCollapsibleState.Collapsed,
      );
      item.description = `(${element.children.length})`;
      return item;
    }

    const leaf = element;
    const item = new TreeItem(leaf.label, TreeItemCollapsibleState.None);
    item.description = leaf.description;
    item.tooltip = leaf.tooltip;
    item.iconPath = leaf.icon;

    if (leaf.startPosition && leaf.sourceUri) {
      const start = leaf.startPosition;
      const end = leaf.endPosition ?? start;
      item.command = {
        command: "vscode.open",
        title: "Go to Source",
        arguments: [
          Uri.parse(leaf.sourceUri),
          {
            selection: new Range(start, end),
          },
        ],
      };
      item.contextValue = "navigable";
    }

    return item;
  }

  async getChildren(element?: AnalysisItem): Promise<AnalysisItem[]> {
    if (element) {
      if (element.type === "category") {
        return element.children;
      }
      return [];
    }

    const editor = window.activeTextEditor;
    const lang = editor?.document?.languageId;

    // For .vue files: fetch full analysis from the LSP
    if (lang === "vue") {
      return this.getVueAnalysis(editor!.document.uri.toString());
    }

    // For .ts/.js files: scan imports client-side for Vue-related items
    if (lang === "typescript" || lang === "javascript") {
      const tsCategories = this.getTsJsAnalysis(editor!);
      const projectOverview = await this.fetchProjectOverview();
      return [...tsCategories, ...projectOverview];
    }

    // For any other file: try using the last known Vue file
    const lastVueUri = this.getLastVueUri();
    if (lastVueUri) {
      return this.getVueAnalysis(lastVueUri);
    }

    // No Vue file context — show project overview only
    const projectOverview = await this.fetchProjectOverview();
    return projectOverview;
  }

  private async getVueAnalysis(sourceUri: string): Promise<AnalysisItem[]> {
    try {
      const [analysis, bindingTypes, projectOverview] = await Promise.all([
        this.getClient().sendRequest(RequestType.GetAnalysis, { uri: sourceUri }),
        this.getClient()
          .sendRequest(RequestType.GetBindingTypes, { uri: sourceUri })
          .catch(() => null),
        this.fetchProjectOverview(),
      ]);

      if (!analysis) return [...projectOverview];

      // Get the source text for byte offset -> Position conversion
      let sourceText: string | undefined;
      const vueDoc = workspace.textDocuments.find(
        (d) => d.uri.toString() === sourceUri,
      );
      if (vueDoc) {
        sourceText = vueDoc.getText();
      }

      const categories = this.buildCategories(
        analysis,
        sourceUri,
        sourceText,
        bindingTypes ?? undefined,
      );
      this.cachedCategories = categories;
      return [...categories, ...projectOverview];
    } catch {
      return [...this.cachedCategories, ...this.cachedProjectOverview];
    }
  }

  private getTsJsAnalysis(
    editor: import("vscode").TextEditor,
  ): AnalysisItem[] {
    const text = editor.document.getText();
    const sourceUri = editor.document.uri.toString();
    const categories: CategoryItem[] = [];

    const imports = this.parseImportsFromText(text);

    // Vue lifecycle hooks
    const lifecycleItems: LeafItem[] = [];
    for (const imp of imports) {
      if (imp.source !== "vue" && !imp.source.startsWith("vue/")) continue;
      for (const binding of imp.bindings) {
        if (VUE_LIFECYCLE_HOOKS.has(binding)) {
          lifecycleItems.push({
            type: "leaf",
            label: binding,
            description: `from "${imp.source}"`,
            tooltip: `Vue lifecycle hook: ${binding}`,
            icon: new ThemeIcon("symbol-event"),
            startPosition: new Position(imp.line, 0),
            sourceUri,
          });
        }
      }
    }
    if (lifecycleItems.length > 0) {
      categories.push({
        type: "category",
        label: "Lifecycle Hooks",
        children: lifecycleItems,
      });
    }

    // Vue composables
    const composableItems: LeafItem[] = [];
    for (const imp of imports) {
      if (imp.source !== "vue" && !imp.source.startsWith("vue/")) continue;
      for (const binding of imp.bindings) {
        if (VUE_COMPOSABLES.has(binding)) {
          composableItems.push({
            type: "leaf",
            label: binding,
            description: `from "${imp.source}"`,
            tooltip: `Vue composable: ${binding}`,
            icon: new ThemeIcon("symbol-method"),
            startPosition: new Position(imp.line, 0),
            sourceUri,
          });
        }
      }
    }
    if (composableItems.length > 0) {
      categories.push({
        type: "category",
        label: "Vue Composables",
        children: composableItems,
      });
    }

    // Component imports (*.vue)
    const componentImports: LeafItem[] = [];
    for (const imp of imports) {
      if (imp.source.endsWith(".vue")) {
        componentImports.push({
          type: "leaf",
          label: imp.bindings[0] ?? imp.source,
          description: `from "${imp.source}"`,
          tooltip: `Vue component import: ${imp.source}`,
          icon: new ThemeIcon("symbol-class"),
          startPosition: new Position(imp.line, 0),
          sourceUri,
        });
      }
    }
    if (componentImports.length > 0) {
      categories.push({
        type: "category",
        label: "Component Imports",
        children: componentImports,
      });
    }

    this.cachedCategories = categories;
    return categories;
  }

  /**
   * Simple client-side import parser for .ts/.js files.
   * Extracts import bindings and sources without requiring a full AST.
   */
  private parseImportsFromText(text: string): ParsedImport[] {
    const imports: ParsedImport[] = [];
    const lines = text.split("\n");

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]!;
      // Match: import { ... } from "..."  or  import ... from "..."
      const match = line.match(
        /^\s*import\s+(?:type\s+)?(?:\{([^}]*)\}|(\w+))\s+from\s+['"](.*?)['"]/,
      );
      if (match) {
        const namedBindings = match[1];
        const defaultBinding = match[2];
        const source = match[3]!;

        const bindings: string[] = [];
        if (namedBindings) {
          for (const b of namedBindings.split(",")) {
            const trimmed = b.trim().replace(/\s+as\s+\w+/, "");
            if (trimmed) bindings.push(trimmed);
          }
        }
        if (defaultBinding) {
          bindings.push(defaultBinding);
        }

        imports.push({ bindings, source, line: i });
      }
    }

    return imports;
  }

  private toPosition(
    source: string | undefined,
    offset: number | undefined,
  ): Position | undefined {
    if (offset === undefined || !source) return undefined;
    return utf16OffsetToPosition(source, offset);
  }

  private buildCategories(
    analysis: FileAnalysisSnapshot,
    sourceUri: string,
    sourceText?: string,
    bindingTypes?: Record<string, string | null>,
  ): CategoryItem[] {
    const categories: CategoryItem[] = [];

    // Imports
    if (analysis.imports?.length > 0) {
      categories.push({
        type: "category",
        label: "Imports",
        children: analysis.imports.map((imp) => {
          const bindingNames = (imp.bindings ?? []).map((b) => b.name).join(", ");
          return {
            type: "leaf" as const,
            label: imp.isTypeOnly
              ? `import type { ${bindingNames} } from "${imp.source}"`
              : `import { ${bindingNames} } from "${imp.source}"`,
            description: imp.isTypeOnly ? "type-only" : "",
            tooltip: [
              `Source: ${imp.source}`,
              `Type-only: ${imp.isTypeOnly}`,
              bindingNames ? `Bindings: ${bindingNames}` : "",
              imp.resolvedCanonicalId
                ? `Resolved: ${imp.resolvedCanonicalId}`
                : "",
            ]
              .filter(Boolean)
              .join("\n"),
            icon: new ThemeIcon("symbol-namespace"),
            startPosition: this.toPosition(sourceText, imp.spanStart),
            endPosition: this.toPosition(sourceText, imp.spanEnd),
            sourceUri,
          };
        }),
      });
    }

    // Type Information (from TSGO) — shown prominently when available
    const hasBindingTypes =
      bindingTypes != null && Object.keys(bindingTypes).length > 0;
    if (hasBindingTypes) {
      const entries = Object.entries(bindingTypes!).filter(
        ([, v]) => v != null,
      );
      if (entries.length > 0) {
        categories.push({
          type: "category",
          label: "Type Information (via tsgo)",
          children: entries.map(([name, type]) => ({
            type: "leaf" as const,
            label: name,
            description: type ?? "",
            tooltip: `${name}: ${type}`,
            icon: new ThemeIcon("symbol-type-parameter"),
            sourceUri,
          })),
        });
      }
    }

    // Bindings
    if (analysis.bindings?.length > 0) {
      categories.push({
        type: "category",
        label: hasBindingTypes ? "Bindings (static)" : "Bindings",
        children: analysis.bindings.map((b) => {
          const reactivity =
            b.reactivityKind !== "None" ? b.reactivityKind.toLowerCase() : "";
          const tsgoType = bindingTypes?.[b.name] ?? null;
          const descParts = [
            b.kind.toLowerCase(),
            reactivity,
            tsgoType,
          ].filter(Boolean);
          return {
            type: "leaf" as const,
            label: b.name,
            description: descParts.join(" · "),
            tooltip: [
              `Name: ${b.name}`,
              `Kind: ${b.kind}`,
              `Reactive: ${b.isReactive}`,
              `Reactivity: ${b.reactivityKind}`,
              b.typeAnnotation ? `Type annotation: ${b.typeAnnotation}` : "",
              tsgoType ? `TSGO type: ${tsgoType}` : "",
            ]
              .filter(Boolean)
              .join("\n"),
            icon: b.isReactive
              ? new ThemeIcon("symbol-variable")
              : new ThemeIcon("symbol-field"),
            startPosition: this.toPosition(sourceText, b.spanStart),
            endPosition: this.toPosition(sourceText, b.spanEnd),
            sourceUri,
          };
        }),
      });
    }

    // Macros
    if (analysis.macros?.length > 0) {
      categories.push({
        type: "category",
        label: "Macros",
        children: analysis.macros.map((m) => ({
          type: "leaf" as const,
          label: m.kind,
          description: m.isTypeBased ? "type-based" : "runtime",
          tooltip: [
            `Kind: ${m.kind}`,
            `Type-based: ${m.isTypeBased}`,
            m.bindingName ? `Binding: ${m.bindingName}` : "",
            m.typeReferences?.length
              ? `Types: ${m.typeReferences.join(", ")}`
              : "",
          ]
            .filter(Boolean)
            .join("\n"),
          icon: new ThemeIcon("symbol-event"),
          startPosition: this.toPosition(sourceText, m.spanStart),
          endPosition: this.toPosition(sourceText, m.spanEnd),
          sourceUri,
        })),
      });
    }

    // Template Components
    if (analysis.template?.components?.length) {
      categories.push({
        type: "category",
        label: "Template Components",
        children: analysis.template.components.map((comp) => ({
          type: "leaf" as const,
          label: comp.name,
          description: comp.importSource
            ? `from "${comp.importSource}"`
            : "(global)",
          tooltip: [
            `Component: ${comp.name}`,
            comp.importSource ? `Import: ${comp.importSource}` : "Global",
            `Dynamic: ${comp.isDynamic}`,
            `Props: ${(comp.props ?? []).map((p) => p.name).join(", ") || "none"}`,
            `Slots: ${(comp.slotsUsed ?? []).join(", ") || "none"}`,
          ]
            .filter(Boolean)
            .join("\n"),
          icon: new ThemeIcon("symbol-class"),
          startPosition: this.toPosition(sourceText, comp.spanStart),
          endPosition: this.toPosition(sourceText, comp.spanEnd),
          sourceUri,
        })),
      });
    }

    // Lifecycle Hooks (extracted from imports with Vue API classification)
    const lifecycleImports = (analysis.imports ?? []).flatMap((imp) =>
      (imp.bindings ?? []).filter(
        (b) =>
          b.vueApi &&
          typeof b.vueApi === "string" &&
          b.vueApi.startsWith("On"),
      ),
    );
    if (lifecycleImports.length > 0) {
      categories.push({
        type: "category",
        label: "Lifecycle Hooks",
        children: lifecycleImports.map((b) => ({
          type: "leaf" as const,
          label: b.name,
          description: "",
          tooltip: `Vue API: ${b.vueApi}`,
          icon: new ThemeIcon("symbol-event"),
          startPosition: this.toPosition(sourceText, b.spanStart),
          endPosition: this.toPosition(sourceText, b.spanEnd),
          sourceUri,
        })),
      });
    }

    // Styles
    if (analysis.styles?.length > 0) {
      categories.push({
        type: "category",
        label: "Styles",
        children: analysis.styles.map((style, i) => {
          const attrs = [
            style.scoped ? "scoped" : "",
            style.isModule ? "module" : "",
            style.lang,
          ]
            .filter(Boolean)
            .join(", ");

          const children: string[] = [];
          if (style.css) {
            for (const cls of style.css.classes ?? []) {
              children.push(`.${cls.name}`);
            }
          }
          for (const vb of style.vBinds ?? []) {
            children.push(`v-bind(${vb.expression})`);
          }

          return {
            type: "leaf" as const,
            label: `Style [${i}] (${attrs})`,
            description: children.length
              ? children.slice(0, 5).join(", ") +
                (children.length > 5 ? "..." : "")
              : "",
            tooltip: [
              `Style block ${i}`,
              `Language: ${style.lang}`,
              `Scoped: ${style.scoped}`,
              `Module: ${style.isModule}${style.moduleName ? ` (${style.moduleName})` : ""}`,
              style.css ? `Selectors: ${(style.css.selectors ?? []).length}` : "",
              style.css ? `Classes: ${(style.css.classes ?? []).map((c) => c.name).join(", ")}` : "",
              style.vBinds?.length
                ? `v-bind: ${style.vBinds.map((v) => v.expression).join(", ")}`
                : "",
            ]
              .filter(Boolean)
              .join("\n"),
            icon: new ThemeIcon("paintcan"),
            sourceUri,
          };
        }),
      });
    }

    // Binding Usage Map — group binding occurrences by name
    const bindingOccs = analysis.template?.bindingOccurrences ?? [];
    if (bindingOccs.length > 0) {
      const grouped = new Map<string, Map<string, number>>();
      for (const occ of bindingOccs) {
        let kindMap = grouped.get(occ.name);
        if (!kindMap) { kindMap = new Map(); grouped.set(occ.name, kindMap); }
        kindMap.set(occ.usageKind, (kindMap.get(occ.usageKind) ?? 0) + 1);
      }
      categories.push({
        type: "category",
        label: "Binding Usage Map",
        children: [...grouped.entries()].map(([name, kindMap]) => {
          const total = [...kindMap.values()].reduce((a, b) => a + b, 0);
          const desc = [...kindMap.entries()].map(([k, c]) => `${k}: ${c}`).join(", ");
          return {
            type: "leaf" as const,
            label: `${name} (${total}x)`,
            description: desc,
            tooltip: `Binding "${name}" used ${total} times in template\n${desc}`,
            icon: new ThemeIcon("symbol-reference"),
            sourceUri,
          };
        }),
      });
    }

    // Event Handlers
    const handlers = analysis.template?.eventHandlers ?? [];
    if (handlers.length > 0) {
      categories.push({
        type: "category",
        label: "Event Handlers",
        children: handlers.map((ev) => ({
          type: "leaf" as const,
          label: `@${ev.eventName}`,
          description: ev.handlerBinding
            ? `→ ${ev.handlerBinding} on <${ev.targetTag}>`
            : `(inline) on <${ev.targetTag}>`,
          tooltip: [
            `Event: ${ev.eventName}`,
            ev.handlerBinding ? `Handler: ${ev.handlerBinding}` : "Inline expression",
            `Target: <${ev.targetTag}>`,
          ].join("\n"),
          icon: new ThemeIcon("zap"),
          startPosition: this.toPosition(sourceText, ev.spanStart),
          endPosition: this.toPosition(sourceText, ev.spanEnd),
          sourceUri,
        })),
      });
    }

    // Template Refs
    const refs = analysis.template?.templateRefs ?? [];
    if (refs.length > 0) {
      categories.push({
        type: "category",
        label: "Template Refs",
        children: refs.map((r) => ({
          type: "leaf" as const,
          label: r.name,
          description: `on <${r.targetTag}>${r.isDynamic ? " (dynamic)" : ""}`,
          tooltip: `Template ref "${r.name}" on <${r.targetTag}>${r.isDynamic ? " (dynamic)" : ""}`,
          icon: new ThemeIcon("symbol-reference"),
          sourceUri,
        })),
      });
    }

    // Slot Definitions
    const slots = analysis.template?.definedSlots ?? [];
    if (slots.length > 0) {
      categories.push({
        type: "category",
        label: "Defined Slots",
        children: slots.map((s) => ({
          type: "leaf" as const,
          label: `#${s.name}`,
          description: s.hasBindings && s.bindingNames?.length
            ? `scoped: ${s.bindingNames.join(", ")}`
            : "no bindings",
          tooltip: [
            `Slot: ${s.name}`,
            s.hasBindings ? `Bindings: ${(s.bindingNames ?? []).join(", ")}` : "No bindings",
          ].join("\n"),
          icon: new ThemeIcon("symbol-interface"),
          startPosition: this.toPosition(sourceText, s.spanStart),
          endPosition: this.toPosition(sourceText, s.spanEnd),
          sourceUri,
        })),
      });
    }

    // Unresolved Bindings
    const unresolved = analysis.template?.unresolvedBindings ?? [];
    if (unresolved.length > 0) {
      categories.push({
        type: "category",
        label: "Unresolved Bindings",
        children: unresolved.map((u) => ({
          type: "leaf" as const,
          label: u.name,
          description: "not defined in script",
          tooltip: `"${u.name}" is used in template but not defined in <script setup>`,
          icon: new ThemeIcon("warning"),
          startPosition: this.toPosition(sourceText, u.spanStart),
          endPosition: this.toPosition(sourceText, u.spanEnd),
          sourceUri,
        })),
      });
    }

    // Prop Definitions
    const propDefs = analysis.template?.propDefinitions ?? [];
    if (propDefs.length > 0) {
      categories.push({
        type: "category",
        label: "Prop Definitions",
        children: propDefs.map((p) => ({
          type: "leaf" as const,
          label: p.name,
          description: [
            p.typeAnnotation ?? "",
            p.isRequired ? "required" : "optional",
            p.hasDefault ? "has default" : "",
          ].filter(Boolean).join(" · "),
          tooltip: [
            `Prop: ${p.name}`,
            p.typeAnnotation ? `Type: ${p.typeAnnotation}` : "",
            `Required: ${p.isRequired}`,
            `Has default: ${p.hasDefault}`,
            `Used in template: ${p.usedInTemplate}`,
            `Used in script: ${p.usedInScript}`,
          ].filter(Boolean).join("\n"),
          icon: new ThemeIcon("symbol-property"),
          startPosition: this.toPosition(sourceText, p.spanStart),
          endPosition: this.toPosition(sourceText, p.spanEnd),
          sourceUri,
        })),
      });
    }

    // Emit Definitions
    const emitDefs = analysis.template?.emitDefinitions ?? [];
    if (emitDefs.length > 0) {
      categories.push({
        type: "category",
        label: "Emit Definitions",
        children: emitDefs.map((e) => ({
          type: "leaf" as const,
          label: e.eventName,
          description: [
            e.isDeclared ? "declared" : "",
            e.hasValidator ? "has validator" : "",
            e.emitLocations?.length ? `${e.emitLocations.length} emit sites` : "",
          ].filter(Boolean).join(" · "),
          tooltip: [
            `Event: ${e.eventName}`,
            `Declared: ${e.isDeclared}`,
            `Has validator: ${e.hasValidator}`,
            e.emitLocations?.length ? `Emit locations: ${e.emitLocations.length}` : "",
          ].filter(Boolean).join("\n"),
          icon: new ThemeIcon("symbol-event"),
          startPosition: this.toPosition(sourceText, e.spanStart),
          endPosition: this.toPosition(sourceText, e.spanEnd),
          sourceUri,
        })),
      });
    }

    // DOM Query Call Sites
    const domQueries = analysis.domQueryCalls ?? [];
    if (domQueries.length > 0) {
      categories.push({
        type: "category",
        label: "DOM Queries",
        children: domQueries.map((q) => ({
          type: "leaf" as const,
          label: q.kind,
          description: `"${q.selectorText}"`,
          tooltip: `${q.kind}("${q.selectorText}")`,
          icon: new ThemeIcon("search"),
          startPosition: this.toPosition(sourceText, q.spanStart),
          endPosition: this.toPosition(sourceText, q.spanEnd),
          sourceUri,
        })),
      });
    }

    // CSS Variable Manipulations
    const cssManips = analysis.cssVarManipulations ?? [];
    if (cssManips.length > 0) {
      categories.push({
        type: "category",
        label: "CSS Variable Manipulations",
        children: cssManips.map((m) => ({
          type: "leaf" as const,
          label: m.kind,
          description: m.valueExpr ? `${m.varName} = ${m.valueExpr}` : m.varName,
          tooltip: `${m.kind}("${m.varName}"${m.valueExpr ? `, ${m.valueExpr}` : ""})`,
          icon: new ThemeIcon("symbol-color"),
          startPosition: this.toPosition(sourceText, m.spanStart),
          endPosition: this.toPosition(sourceText, m.spanEnd),
          sourceUri,
        })),
      });
    }

    // Vue API Calls (grouped by category)
    const apiCalls = analysis.vueApiCalls ?? [];
    if (apiCalls.length > 0) {
      categories.push({
        type: "category",
        label: "Vue API Calls",
        children: apiCalls.map((c) => ({
          type: "leaf" as const,
          label: c.api,
          description: c.argValue ? `"${c.argValue}"` : "",
          tooltip: [
            `API: ${c.api}`,
            c.argValue ? `Argument: "${c.argValue}"` : "",
            c.isAsyncCallback ? "Async callback" : "",
          ].filter(Boolean).join("\n"),
          icon: new ThemeIcon("symbol-method"),
          startPosition: this.toPosition(sourceText, c.spanStart),
          endPosition: this.toPosition(sourceText, c.spanEnd),
          sourceUri,
        })),
      });
    }

    return categories;
  }

  /**
   * Fetch project overview from the LSP and build categories.
   */
  private async fetchProjectOverview(): Promise<CategoryItem[]> {
    try {
      const overview = await this.getClient().sendRequest(
        RequestType.GetProjectOverview,
        {} as Record<string, never>,
      );
      if (!overview) return this.cachedProjectOverview;

      const categories: CategoryItem[] = [];

      // Stats summary
      const statsItems: LeafItem[] = [];
      statsItems.push({
        type: "leaf",
        label: `${overview.stats.totalVueFiles} Vue files`,
        description: "",
        tooltip: "Total number of .vue files tracked by the host",
        icon: new ThemeIcon("file-code"),
      });
      statsItems.push({
        type: "leaf",
        label: `${overview.stats.totalComponents} component usages`,
        description: "",
        tooltip: "Total component tag usages across all templates",
        icon: new ThemeIcon("symbol-class"),
      });
      if (overview.stats.filesWithScopedStyles > 0) {
        statsItems.push({
          type: "leaf",
          label: `${overview.stats.filesWithScopedStyles} files with scoped styles`,
          description: "",
          tooltip: "Files using <style scoped>",
          icon: new ThemeIcon("paintcan"),
        });
      }
      categories.push({
        type: "category",
        label: "Project Overview",
        children: statsItems,
      });

      // Component graph
      if (overview.componentGraph.length > 0) {
        const graphItems: LeafItem[] = overview.componentGraph.map((edge) => ({
          type: "leaf" as const,
          label: basename(edge.file),
          description: `uses: ${edge.usesComponents.join(", ")}`,
          tooltip: `${edge.file}\nComponents: ${edge.usesComponents.join(", ")}`,
          icon: new ThemeIcon("type-hierarchy"),
        }));
        categories.push({
          type: "category",
          label: "Component Graph",
          children: graphItems,
        });
      }

      // File index
      if (overview.files.length > 0) {
        const fileItems: LeafItem[] = overview.files
          .filter((f) => f.kind === "vue")
          .map((f) => ({
            type: "leaf" as const,
            label: basename(f.path),
            description: f.kind,
            tooltip: f.path,
            icon: new ThemeIcon("file-code"),
          }));
        if (fileItems.length > 0) {
          categories.push({
            type: "category",
            label: "Vue Files",
            children: fileItems,
          });
        }
      }

      this.cachedProjectOverview = categories;
      return categories;
    } catch {
      return this.cachedProjectOverview;
    }
  }

  dispose(): void {
    this.subscriptions.forEach((d) => d.dispose());
    this.subscriptions.length = 0;
  }
}
