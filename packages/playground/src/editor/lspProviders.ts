/**
 * Monaco LSP providers powered by Verter analysis data.
 * Ports hover and completion logic from crates/verter_lsp/src/features/.
 */
import * as monaco from "monaco-editor-core";
import type { Store } from "../core/store";
import { hoverForWord, collectCompletions, isOffsetInScriptBlock } from "./analysisHelpers";

// ── TypeScript service integration interface ──

export interface TypeScriptServiceBridge {
  getHover(filename: string, offset: number): Promise<string | null>;
  getCompletions(
    filename: string,
    offset: number,
  ): Promise<Array<{ label: string; kind: number; detail?: string; insertText?: string }>>;
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

// ── Registration ──

export function registerLspProviders(
  store: Store,
  tsBridge?: TypeScriptServiceBridge,
): monaco.IDisposable[] {
  const disposables: monaco.IDisposable[] = [];

  // Hover provider
  disposables.push(
    monaco.languages.registerHoverProvider("vue", {
      async provideHover(model, position) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;

        // Try Verter analysis first
        if (analysis) {
          const word = model.getWordAtPosition(position);
          if (word) {
            const content = hoverForWord(word.word, analysis);
            if (content) {
              return {
                range: new monaco.Range(
                  position.lineNumber,
                  word.startColumn,
                  position.lineNumber,
                  word.endColumn,
                ),
                contents: [{ value: content }],
              };
            }
          }
        }

        // Fall back to TypeScript service
        if (tsBridge && file) {
          const offset = model.getOffsetAt(position);
          const tsContent = await tsBridge.getHover(file.filename, offset);
          if (tsContent) {
            const word = model.getWordAtPosition(position);
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
            return { range, contents: [{ value: tsContent }] };
          }
        }

        return null;
      },
    }),
  );

  // Completion provider
  disposables.push(
    monaco.languages.registerCompletionItemProvider("vue", {
      triggerCharacters: [".", '"', "'", "/", "@", "<"],
      async provideCompletionItems(model, position) {
        const file = store.activeFile;
        const analysis = file?.compiled.analysis;
        const items: monaco.languages.CompletionItem[] = [];

        const wordRange = model.getWordUntilPosition(position);
        const range = new monaco.Range(
          position.lineNumber,
          wordRange.startColumn,
          position.lineNumber,
          wordRange.endColumn,
        );

        if (analysis) {
          const source = model.getValue();
          const offset = model.getOffsetAt(position);
          const isInScript = isOffsetInScriptBlock(source, offset);

          const entries = collectCompletions(analysis, isInScript);
          for (const entry of entries) {
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
          const offset = model.getOffsetAt(position);
          const tsItems = await tsBridge.getCompletions(file.filename, offset);
          for (const tsItem of tsItems) {
            if (items.some((i) => (i.label as string) === tsItem.label)) continue;
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

  return disposables;
}
