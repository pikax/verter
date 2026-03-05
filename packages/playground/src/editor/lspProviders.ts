/**
 * Monaco LSP providers powered by Verter analysis data.
 * Ports hover and completion logic from crates/verter_lsp/src/features/.
 */
import * as monaco from "monaco-editor-core";
import type { Store } from "../core/store";
import { hoverForWord, collectCompletions, isOffsetInScriptBlock } from "./analysisHelpers";
import {
  collectTemplateCompletions,
  collectTemplateInterpolationCompletions,
  isOffsetInTemplateBlock,
  type TemplateCompletion,
} from "./templateIde";
import { getCodeActions, getDocumentSymbols, type HostDocumentSymbol } from "../core/compiler";
import { computeCodeLenses, computeBindingInlayHints } from "./decorations";

// ── TypeScript service integration interface ──

export interface TypeScriptServiceBridge {
  getHover(filename: string, offset: number): Promise<string | null>;
  getCompletions(
    filename: string,
    offset: number,
  ): Promise<Array<{ label: string; kind: number; detail?: string; insertText?: string }>>;
  getDefinition?: (
    filename: string,
    offset: number,
  ) => Promise<Array<{ start: number; end: number }>>;
  getReferences?: (
    filename: string,
    offset: number,
  ) => Promise<Array<{ start: number; end: number; isDefinition: boolean }>>;
  getDocumentHighlights?: (
    filename: string,
    offset: number,
  ) => Promise<Array<{ start: number; end: number }>>;
  getRenameLocations?: (
    filename: string,
    offset: number,
  ) => Promise<{
    canRename: boolean;
    rejectReason?: string;
    triggerSpan: { start: number; end: number } | null;
    locations: Array<{ start: number; end: number }>;
  }>;
  /** Ensure the worker has the latest TSX file and source map before LSP operations. */
  ensureTsxCurrent?: (
    vueFilename: string,
    tsxCode: string,
    vueCode: string,
    sourceMapJson: string | null,
    destructuredBlock?: import("../core/types").DestructuredBlockMeta | null,
  ) => Promise<void>;
}

// Mapping from analysis completion kind to Monaco CompletionItemKind
const KIND_MAP: Record<string, monaco.languages.CompletionItemKind> = {
  Constant: monaco.languages.CompletionItemKind.Constant,
  Variable: monaco.languages.CompletionItemKind.Variable,
  Function: monaco.languages.CompletionItemKind.Function,
  Class: monaco.languages.CompletionItemKind.Class,
  Module: monaco.languages.CompletionItemKind.Module,
  TypeParameter: monaco.languages.CompletionItemKind.TypeParameter,
};

function templateCompletionKind(kind: TemplateCompletion["kind"]): monaco.languages.CompletionItemKind {
  switch (kind) {
    case "tag":
      return monaco.languages.CompletionItemKind.Class;
    case "directive":
      return monaco.languages.CompletionItemKind.Keyword;
    case "attribute":
      return monaco.languages.CompletionItemKind.Property;
    case "symbol":
    default:
      return monaco.languages.CompletionItemKind.Variable;
  }
}

function pushTemplateCompletion(
  items: monaco.languages.CompletionItem[],
  model: monaco.editor.ITextModel,
  range: monaco.Range,
  completion: TemplateCompletion,
): void {
  const additionalTextEdits = completion.importEdit
    ? (() => {
        const pos = model.getPositionAt(completion.importEdit.offset);
        const editRange = new monaco.Range(pos.lineNumber, pos.column, pos.lineNumber, pos.column);
        return [{ range: editRange, text: completion.importEdit.text }];
      })()
    : undefined;

  items.push({
    label: completion.label,
    kind: templateCompletionKind(completion.kind),
    detail: completion.detail,
    insertText: completion.insertText,
    range,
    sortText: completion.sortText,
    insertTextRules: completion.isSnippet
      ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
      : undefined,
    additionalTextEdits,
  } as unknown as monaco.languages.CompletionItem);
}

function mappedSpanToRange(
  model: monaco.editor.ITextModel,
  span: { start: number; end: number },
): monaco.Range {
  const start = model.getPositionAt(span.start);
  const end = model.getPositionAt(span.end);
  return new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column);
}

// ── Registration ──

export function registerLspProviders(
  store: Store,
  tsBridge?: TypeScriptServiceBridge,
): monaco.IDisposable[] {
  const disposables: monaco.IDisposable[] = [];

  /** Ensure TSX compilation and worker sync are current before any LSP operation. */
  async function ensureTsxSynced() {
    const file = store.activeFile;
    if (!tsBridge?.ensureTsxCurrent || !file) return;
    // Force recompile — handles the race where the user just typed but
    // Vue's async watcher hasn't triggered compileFile yet.
    // The WASM compile is synchronous (~2ms); the host skips if source unchanged.
    await store.recompile();
    const tsxCode = file.compiled.types;
    if (!tsxCode) return;
    const sourceMap = file.compiled.typesSourceMap || null;
    await tsBridge.ensureTsxCurrent(file.filename, tsxCode, file.code, sourceMap, file.compiled.destructuredBlock);
  }

  // Hover provider
  disposables.push(
    monaco.languages.registerHoverProvider("vue", {
      async provideHover(model, position) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;
        const word = model.getWordAtPosition(position);
        const offset = model.getOffsetAt(position);

        const range = word
          ? new monaco.Range(
              position.lineNumber,
              word.startColumn,
              position.lineNumber,
              word.endColumn,
            )
          : new monaco.Range(
              position.lineNumber,
              position.column,
              position.lineNumber,
              position.column,
            );

        // Collect hover content from both sources
        const contents: monaco.IMarkdownString[] = [];

        // TS type info first (primary)
        if (tsBridge && file) {
          await ensureTsxSynced();
          const tsContent = await tsBridge.getHover(file.filename, offset);
          if (tsContent) contents.push({ value: tsContent });
        }

        // Analysis info second (supplementary)
        if (analysis && word) {
          const analysisContent = hoverForWord(word.word, analysis);
          if (analysisContent) contents.push({ value: analysisContent });
        }

        if (contents.length === 0) return null;
        return { range, contents };
      },
    }),
  );

  // Completion provider
  disposables.push(
    monaco.languages.registerCompletionItemProvider("vue", {
      triggerCharacters: [".", '"', "'", "/", "@", "<", ":", "$"],
      async provideCompletionItems(model, position) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;
        const items: monaco.languages.CompletionItem[] = [];
        const seenLabels = new Set<string>();

        const wordRange = model.getWordUntilPosition(position);
        const range = new monaco.Range(
          position.lineNumber,
          wordRange.startColumn,
          position.lineNumber,
          wordRange.endColumn,
        );

        const source = model.getValue();
        const offset = model.getOffsetAt(position);
        const isInScript = isOffsetInScriptBlock(source, offset);
        const isInTemplate = !isInScript && isOffsetInTemplateBlock(source, offset);

        // Detect member access: cursor right after `.` → only TS member completions
        const lineContent = model.getLineContent(position.lineNumber);
        const charBeforeIdx = position.column - 2; // 0-based, column is 1-based
        const isMemberAccess = charBeforeIdx >= 0 && lineContent[charBeforeIdx] === ".";

        let templateCompletionCount = 0;

        if (isInTemplate && !isMemberAccess) {
          const openFilenames = Object.keys(store.files as Record<string, unknown>);
          const templateCompletions = collectTemplateCompletions({
            source,
            offset,
            activeFilename: store.activeFilename,
            openFilenames,
            analysis,
          });

          for (const completion of templateCompletions) {
            if (seenLabels.has(completion.label)) continue;
            seenLabels.add(completion.label);
            pushTemplateCompletion(items, model, range, completion);
            templateCompletionCount += 1;
          }

          const interpolationCompletions = collectTemplateInterpolationCompletions({
            source,
            offset,
            activeFilename: store.activeFilename,
            openFilenames,
            analysis,
          });

          for (const completion of interpolationCompletions) {
            if (seenLabels.has(completion.label)) continue;
            seenLabels.add(completion.label);
            pushTemplateCompletion(items, model, range, completion);
            templateCompletionCount += 1;
          }
        }

        // Analysis completions: identifiers/bindings — skip for member access (e.g. `count.`)
        if (analysis && !isMemberAccess && (templateCompletionCount === 0 || isInScript)) {
          const entries = collectCompletions(analysis, isInScript);
          for (const entry of entries) {
            if (seenLabels.has(entry.label)) continue;
            seenLabels.add(entry.label);
            items.push({
              label: entry.label,
              kind: KIND_MAP[entry.kind] ?? monaco.languages.CompletionItemKind.Variable,
              detail: entry.detail,
              insertText: entry.label,
              range,
            } as unknown as monaco.languages.CompletionItem);
          }
        }

        // Merge TS completions
        if (tsBridge && file) {
          await ensureTsxSynced();
          const tsItems = await tsBridge.getCompletions(file.filename, offset);
          for (const tsItem of tsItems) {
            if (seenLabels.has(tsItem.label)) continue;
            seenLabels.add(tsItem.label);
            items.push({
              label: tsItem.label,
              kind: tsItem.kind,
              detail: tsItem.detail,
              insertText: tsItem.insertText ?? tsItem.label,
              range,
            } as unknown as monaco.languages.CompletionItem);
          }
        }

        return { suggestions: items };
      },
    }),
  );

  // Go to definition
  if (tsBridge?.getDefinition) {
    disposables.push(
      monaco.languages.registerDefinitionProvider("vue", {
        async provideDefinition(model, position) {
          const file = store.activeFile;
          if (!file) return null;
          await ensureTsxSynced();
          const offset = model.getOffsetAt(position);
          const defs = await tsBridge.getDefinition!(file.filename, offset);
          if (defs.length === 0) return null;
          return defs.map((def) => ({
            uri: model.uri,
            range: mappedSpanToRange(model, def),
          })) as unknown as monaco.languages.Location[];
        },
      }),
    );
  }

  // Find all references
  if (tsBridge?.getReferences) {
    disposables.push(
      monaco.languages.registerReferenceProvider("vue", {
        async provideReferences(model, position) {
          const file = store.activeFile;
          if (!file) return [];
          await ensureTsxSynced();
          const offset = model.getOffsetAt(position);
          const refs = await tsBridge.getReferences!(file.filename, offset);
          return refs.map((ref) => ({
            uri: model.uri,
            range: mappedSpanToRange(model, ref),
          })) as unknown as monaco.languages.Location[];
        },
      }),
    );
  }

  // Document highlights
  if (tsBridge?.getDocumentHighlights) {
    disposables.push(
      monaco.languages.registerDocumentHighlightProvider("vue", {
        async provideDocumentHighlights(model, position) {
          const file = store.activeFile;
          if (!file) return [];
          await ensureTsxSynced();
          const offset = model.getOffsetAt(position);
          const highlights = await tsBridge.getDocumentHighlights!(file.filename, offset);
          return highlights.map((span) => ({
            range: mappedSpanToRange(model, span),
            kind: monaco.languages.DocumentHighlightKind.Read,
          })) as unknown as monaco.languages.DocumentHighlight[];
        },
      }),
    );
  }

  // Rename symbol
  if (tsBridge?.getRenameLocations) {
    disposables.push(
      monaco.languages.registerRenameProvider("vue", {
        async resolveRenameLocation(model, position) {
          const file = store.activeFile;
          if (!file) return null;
          await ensureTsxSynced();
          const offset = model.getOffsetAt(position);
          const rename = await tsBridge.getRenameLocations!(file.filename, offset);
          if (!rename.canRename || !rename.triggerSpan) return null;
          const range = mappedSpanToRange(model, rename.triggerSpan);
          return {
            range,
            text: model.getValueInRange(range),
          } as unknown as monaco.languages.RenameLocation;
        },
        async provideRenameEdits(model, position, newName) {
          const file = store.activeFile;
          if (!file) {
            return {
              edits: [],
              rejectReason: "No active file",
            };
          }

          await ensureTsxSynced();
          const offset = model.getOffsetAt(position);
          const rename = await tsBridge.getRenameLocations!(file.filename, offset);
          if (!rename.canRename) {
            return {
              edits: [],
              rejectReason: rename.rejectReason ?? "Symbol cannot be renamed here",
            } as unknown as monaco.languages.WorkspaceEdit;
          }

          const edits = rename.locations.map((loc) => ({
            resource: model.uri,
            edit: {
              range: mappedSpanToRange(model, loc),
              text: newName,
            },
          }));

          return { edits } as unknown as monaco.languages.WorkspaceEdit;
        },
      }),
    );
  }

  // Code action provider (quick fixes from Verter lint rules)
  disposables.push(
    monaco.languages.registerCodeActionProvider("vue", {
      provideCodeActions(model, range) {
        const file = store.activeFile;
        if (!file) return { actions: [], dispose() {} };
        const offset = model.getOffsetAt(range.getStartPosition());
        const actions = getCodeActions(file.filename, offset);
        if (actions.length === 0) return { actions: [], dispose() {} };

        const monacoActions: monaco.languages.CodeAction[] = actions.map((action) => ({
          title: action.title,
          kind: action.kind === "quickfix" ? "quickfix" : action.kind === "refactor" ? "refactor" : "source",
          isPreferred: action.isPreferred,
          diagnostics: action.diagnosticRule
            ? [
                {
                  severity: monaco.MarkerSeverity.Warning,
                  message: action.diagnosticRule,
                  startLineNumber: range.startLineNumber,
                  startColumn: range.startColumn,
                  endLineNumber: range.endLineNumber,
                  endColumn: range.endColumn,
                },
              ]
            : undefined,
          edit: {
            edits: action.edits.map((edit) => {
              const editStart = model.getPositionAt(edit.spanStart);
              const editEnd = model.getPositionAt(edit.spanEnd);
              return {
                resource: model.uri,
                textEdit: {
                  range: new monaco.Range(
                    editStart.lineNumber,
                    editStart.column,
                    editEnd.lineNumber,
                    editEnd.column,
                  ),
                  text: edit.newText,
                },
                versionId: model.getVersionId(),
              };
            }),
          },
        }));

        return { actions: monacoActions, dispose() {} };
      },
    }),
  );

  // Document symbol provider (outline / Ctrl+Shift+O)
  disposables.push(
    monaco.languages.registerDocumentSymbolProvider("vue", {
      displayName: "Verter",
      provideDocumentSymbols(model) {
        const file = store.activeFile;
        if (!file) return [];
        const symbols = getDocumentSymbols(file.filename);
        if (symbols.length === 0) return [];

        function mapSymbol(sym: HostDocumentSymbol): monaco.languages.DocumentSymbol {
          const start = model.getPositionAt(sym.spanStart);
          const end = model.getPositionAt(sym.spanEnd);
          const selStart = model.getPositionAt(sym.selectionStart);
          const selEnd = model.getPositionAt(sym.selectionEnd);
          return {
            name: sym.name,
            detail: sym.detail ?? "",
            kind: sym.kind as monaco.languages.SymbolKind,
            tags: [],
            range: new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column),
            selectionRange: new monaco.Range(
              selStart.lineNumber,
              selStart.column,
              selEnd.lineNumber,
              selEnd.column,
            ),
            children: sym.children.map(mapSymbol),
          };
        }

        return symbols.map(mapSymbol);
      },
    }),
  );

  // CodeLens provider (block summaries)
  disposables.push(
    monaco.languages.registerCodeLensProvider("vue", {
      provideCodeLenses(model) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;
        if (!file || !analysis) return { lenses: [], dispose() {} };

        const lenses = computeCodeLenses(file.code, analysis);
        const monacoLenses: monaco.languages.CodeLens[] = lenses.map((lens) => ({
          range: new monaco.Range(lens.line, 1, lens.line, 1),
          command: {
            id: "",
            title: lens.title,
          },
        }));

        return { lenses: monacoLenses, dispose() {} };
      },
    }),
  );

  // Inlay hints provider (per-binding type hints)
  disposables.push(
    monaco.languages.registerInlayHintsProvider("vue", {
      provideInlayHints(model) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;
        if (!file || !analysis) return { hints: [], dispose() {} };

        const hints = computeBindingInlayHints(analysis);
        const monacoHints: monaco.languages.InlayHint[] = hints.map((hint) => ({
          position: model.getPositionAt(hint.position),
          label: hint.label,
          kind: hint.kind === "type"
            ? monaco.languages.InlayHintKind.Type
            : monaco.languages.InlayHintKind.Parameter,
        }));

        return { hints: monacoHints, dispose() {} };
      },
    }),
  );

  return disposables;
}
