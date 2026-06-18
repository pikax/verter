/**
 * In-process TypeScript language service for the VS Code extension.
 *
 * Adapted from `@verter/ts-service` (Experiment D) — same LanguageServiceHost
 * and request dispatcher, but runs directly in the extension host process
 * instead of over TCP. The Rust LSP sends `$/verter/tsQuery` requests which
 * are dispatched to `handleQuery()`.
 *
 * TypeScript is resolved from the workspace (via `createRequire`) so the
 * language service uses the project's own TS version.
 */

import { createRequire } from "module";
import { join } from "path";
import type * as ts from "typescript";

export class ExtensionTsService {
  private ts!: typeof ts;
  private service!: ts.LanguageService;
  private fileSnapshots = new Map<string, ts.IScriptSnapshot>();
  private fileVersions = new Map<string, number>();
  private openFiles = new Set<string>();
  private workspaceRoot: string;
  private initialized = false;

  constructor(workspaceRoot: string) {
    this.workspaceRoot = workspaceRoot;
  }

  private ensureInitialized(): void {
    if (this.initialized) return;
    this.initialized = true;

    // Resolve TypeScript from the workspace, fall back to bundled
    let tsModule: typeof ts;
    try {
      const wsRequire = createRequire(join(this.workspaceRoot, "package.json"));
      tsModule = wsRequire("typescript") as typeof ts;
    } catch {
      tsModule = require("typescript") as typeof ts;
    }
    this.ts = tsModule;

    const compilerOptions: ts.CompilerOptions = this.resolveCompilerOptions();

    const host: ts.LanguageServiceHost = {
      getScriptFileNames: () => [...this.openFiles],
      getScriptVersion: (fileName) => String(this.fileVersions.get(fileName) ?? 0),
      getScriptSnapshot: (fileName) => {
        if (this.fileSnapshots.has(fileName)) return this.fileSnapshots.get(fileName)!;
        try {
          const content = this.ts.sys.readFile(fileName);
          if (content !== undefined) {
            const snap = this.ts.ScriptSnapshot.fromString(content);
            this.fileSnapshots.set(fileName, snap);
            return snap;
          }
        } catch {
          // Ignore read errors
        }
        return undefined;
      },
      getCurrentDirectory: () => this.workspaceRoot,
      getCompilationSettings: () => compilerOptions,
      getDefaultLibFileName: this.ts.getDefaultLibFilePath,
      fileExists: this.ts.sys.fileExists,
      readFile: this.ts.sys.readFile,
      readDirectory: this.ts.sys.readDirectory,
      directoryExists: this.ts.sys.directoryExists,
      getDirectories: this.ts.sys.getDirectories,
    };

    this.service = this.ts.createLanguageService(host, this.ts.createDocumentRegistry());
  }

  private resolveCompilerOptions(): ts.CompilerOptions {
    // Try to find tsconfig.json in the workspace root
    const tsconfigPath = this.ts.findConfigFile(
      this.workspaceRoot,
      this.ts.sys.fileExists,
      "tsconfig.json",
    );

    if (tsconfigPath) {
      const configFile = this.ts.readConfigFile(tsconfigPath, this.ts.sys.readFile);
      if (!configFile.error) {
        const parsed = this.ts.parseJsonConfigFileContent(
          configFile.config,
          this.ts.sys,
          this.workspaceRoot,
        );
        return parsed.options;
      }
    }

    // Default options matching ts-service/server.ts
    return {
      target: this.ts.ScriptTarget.ESNext,
      module: this.ts.ModuleKind.ESNext,
      moduleResolution: this.ts.ModuleResolutionKind.Bundler,
      jsx: this.ts.JsxEmit.ReactJSX,
      jsxImportSource: "vue",
      strict: true,
      allowJs: true,
      checkJs: false,
      noEmit: true,
      allowNonTsExtensions: true,
    };
  }

  /**
   * Handle a tsserver-format query. Called by the `$/verter/tsQuery` handler.
   * Returns the response body (same shape as tsserver responses).
   */
  handleQuery(command: string, args: Record<string, unknown>): unknown {
    this.ensureInitialized();

    switch (command) {
      case "configure":
        return {};

      case "compilerOptionsForInferredProjects":
        return true;

      case "open": {
        const file = args.file as string;
        const content = args.fileContent as string | undefined;
        this.openFiles.add(file);
        if (content !== undefined) {
          this.fileSnapshots.set(file, this.ts.ScriptSnapshot.fromString(content));
          this.fileVersions.set(file, (this.fileVersions.get(file) ?? 0) + 1);
        }
        return {};
      }

      case "updateOpen": {
        const openEntries = (args.openFiles ?? []) as Array<{
          file: string;
          fileContent?: string;
        }>;
        const changedEntries = (args.changedFiles ?? []) as Array<{
          fileName: string;
          textChanges?: Array<{
            start: { line: number; offset: number };
            end: { line: number; offset: number };
            newText: string;
          }>;
        }>;
        const closedEntries = (args.closedFiles ?? []) as string[];

        for (const entry of openEntries) {
          this.openFiles.add(entry.file);
          if (entry.fileContent !== undefined) {
            this.fileSnapshots.set(
              entry.file,
              this.ts.ScriptSnapshot.fromString(entry.fileContent),
            );
            this.fileVersions.set(entry.file, (this.fileVersions.get(entry.file) ?? 0) + 1);
          }
        }

        for (const entry of changedEntries) {
          const currentSnap = this.fileSnapshots.get(entry.fileName);
          if (currentSnap && entry.textChanges?.length) {
            let text = currentSnap.getText(0, currentSnap.getLength());
            const changes = [...entry.textChanges].sort(
              (a, b) => b.start.line - a.start.line || b.start.offset - a.start.offset,
            );
            for (const change of changes) {
              const startOffset = this.positionToOffset(
                text,
                change.start.line,
                change.start.offset,
              );
              const endOffset = this.positionToOffset(text, change.end.line, change.end.offset);
              text = text.slice(0, startOffset) + change.newText + text.slice(endOffset);
            }
            this.fileSnapshots.set(entry.fileName, this.ts.ScriptSnapshot.fromString(text));
            this.fileVersions.set(entry.fileName, (this.fileVersions.get(entry.fileName) ?? 0) + 1);
          }
        }

        for (const file of closedEntries) {
          this.openFiles.delete(file);
        }

        return true;
      }

      case "close": {
        const file = args.file as string;
        this.openFiles.delete(file);
        return {};
      }

      case "quickinfo": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const info = this.service.getQuickInfoAtPosition(file, offset);
        if (!info) return undefined;
        const display = this.ts.displayPartsToString(info.displayParts);
        const docs = this.ts.displayPartsToString(info.documentation);
        const start = this.offsetToPosition(text, info.textSpan.start);
        const end = this.offsetToPosition(text, info.textSpan.start + info.textSpan.length);
        return {
          kind: info.kind,
          kindModifiers: info.kindModifiers,
          start: { line: start.line, offset: start.offset },
          end: { line: end.line, offset: end.offset },
          displayString: display,
          documentation: docs,
          tags: info.tags?.map((t) => ({
            name: t.name,
            text: t.text ? t.text.map((p) => p.text).join("") : undefined,
          })),
        };
      }

      case "completionInfo": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const completions = this.service.getCompletionsAtPosition(file, offset, {
          includeCompletionsForModuleExports: true,
          includeCompletionsWithInsertText: true,
        });
        if (!completions) return undefined;
        return {
          isGlobalCompletion: completions.isGlobalCompletion,
          isMemberCompletion: completions.isMemberCompletion,
          entries: completions.entries.map((e) => ({
            name: e.name,
            kind: e.kind,
            kindModifiers: e.kindModifiers,
            sortText: e.sortText,
            insertText: e.insertText,
            replacementSpan: e.replacementSpan
              ? this.spanToRange(text, e.replacementSpan)
              : undefined,
            // Auto-import resolve key: a module-export entry carries `source`
            // (+ the optional opaque `data` blob), which `getCompletionEntryDetails`
            // keys the auto-import `codeActions` lookup on. Forwarding them lets
            // the provider re-issue `completionEntryDetails` for the selected
            // entry — without them the extension provider could never resolve an
            // auto-import. `hasAction` is NOT forwarded: it is purely an output
            // hint (not an input to the details lookup), and the auto-import
            // resolve contract is `source`/`data` only — an auto-import entry
            // always carries `source`. The other `hasAction:true` shapes
            // (class-member snippet completions, missing-comma insertion,
            // type-only-alias wrappers) are a DIFFERENT action class this
            // resolve path deliberately does not route as imports. See
            // `crates/verter_type_runtime/src/protocol.rs` (`is_actionable`) and
            // `docs/arch/provider-completion-resolve-design.md`.
            source: e.source,
            data: e.data,
            labelDetails: e.labelDetails,
            sourceDisplay: e.sourceDisplay
              ? this.ts.displayPartsToString(e.sourceDisplay)
              : undefined,
          })),
        };
      }

      case "completionEntryDetails": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        // The selected entries to resolve, each `{ name, source?, data? }` —
        // `source`/`data` route an external-module entry to the right symbol.
        const entryNames = (args.entryNames as unknown[]) ?? [];
        const details = entryNames.map((raw) => {
          const entry = raw as { name?: string; source?: string; data?: unknown };
          const name = entry.name ?? "";
          // `formatOptions` MUST be provided: when resolving an auto-import
          // (external-module) entry, TypeScript builds the import-insertion
          // `codeActions` through its formatter, which dereferences the format
          // settings. Passing `undefined` crashes the import code-action builder
          // (`Cannot read properties of undefined (reading 'options')`), so the
          // extension provider could never resolve an auto-import edit. Default
          // format settings are sufficient — the inserted import is normalized
          // by the shared tsserver-family resolve mapper downstream.
          const detail = this.service.getCompletionEntryDetails(
            file,
            offset,
            name,
            this.ts.getDefaultFormatCodeSettings("\n"),
            entry.source,
            undefined,
            entry.data as ts.CompletionEntryData | undefined,
          );
          if (!detail) return { name };
          return {
            name: detail.name,
            kind: detail.kind,
            kindModifiers: detail.kindModifiers,
            displayParts: detail.displayParts?.map((p) => ({ text: p.text, kind: p.kind })),
            documentation: detail.documentation?.map((p) => ({ text: p.text, kind: p.kind })),
            tags: detail.tags?.map((t) => ({
              name: t.name,
              text: t.text ? t.text.map((p) => p.text).join("") : undefined,
            })),
            // The auto-import edit set: each code action's `changes` are tsserver
            // `{ fileName, textChanges }` with 1-based line/offset positions, the
            // shape the shared tsserver-family resolve mapper consumes.
            codeActions: detail.codeActions?.map((action) => ({
              description: action.description,
              changes: action.changes.map((change) => ({
                fileName: change.fileName,
                textChanges: change.textChanges.map((tc) => {
                  const changeText = this.getFileText(change.fileName);
                  return {
                    start: this.offsetToPosition(changeText, tc.span.start),
                    end: this.offsetToPosition(changeText, tc.span.start + tc.span.length),
                    newText: tc.newText,
                  };
                }),
              })),
            })),
          };
        });
        return details;
      }

      case "definition":
      case "typeDefinition": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const fn_ =
          command === "definition"
            ? this.service.getDefinitionAtPosition
            : this.service.getTypeDefinitionAtPosition;
        const defs = fn_.call(this.service, file, offset);
        return (defs ?? []).map((d) => ({
          file: d.fileName,
          start: this.offsetToPosition(this.getFileText(d.fileName), d.textSpan.start),
          end: this.offsetToPosition(
            this.getFileText(d.fileName),
            d.textSpan.start + d.textSpan.length,
          ),
        }));
      }

      case "references": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const refs = this.service.getReferencesAtPosition(file, offset);
        return {
          refs: (refs ?? []).map((r) => ({
            file: r.fileName,
            start: this.offsetToPosition(this.getFileText(r.fileName), r.textSpan.start),
            end: this.offsetToPosition(
              this.getFileText(r.fileName),
              r.textSpan.start + r.textSpan.length,
            ),
            isDefinition: (r as unknown as Record<string, unknown>).isDefinition ?? false,
            isWriteAccess: r.isWriteAccess,
          })),
        };
      }

      case "rename": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const locations = this.service.findRenameLocations(file, offset, false, false);
        const locArray: ts.RenameLocation[] = locations ? [...locations] : [];
        return {
          info: {
            canRename: !!locations,
            localizedErrorMessage: locations ? "" : "Cannot rename",
          },
          locs: this.groupBy(locArray, (r) => r.fileName).map(([locFile, spans]) => ({
            file: locFile,
            locs: spans.map((s) => ({
              start: this.offsetToPosition(this.getFileText(locFile), s.textSpan.start),
              end: this.offsetToPosition(
                this.getFileText(locFile),
                s.textSpan.start + s.textSpan.length,
              ),
            })),
          })),
        };
      }

      case "signatureHelp": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const help = this.service.getSignatureHelpItems(file, offset, {});
        if (!help) return undefined;
        return {
          items: help.items.map((item) => ({
            isVariadic: item.isVariadic,
            prefixDisplayParts: item.prefixDisplayParts,
            suffixDisplayParts: item.suffixDisplayParts,
            separatorDisplayParts: item.separatorDisplayParts,
            parameters: item.parameters.map((p) => ({
              name: p.name,
              documentation: p.documentation,
              displayParts: p.displayParts,
              isOptional: p.isOptional,
            })),
            documentation: item.documentation,
          })),
          selectedItemIndex: help.selectedItemIndex,
          argumentIndex: help.argumentIndex,
          argumentCount: help.argumentCount,
        };
      }

      case "semanticDiagnosticsSync": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const diags = this.service.getSemanticDiagnostics(file);
        return diags.map((d) => ({
          start: d.start !== undefined ? this.offsetToPosition(text, d.start) : undefined,
          end:
            d.start !== undefined && d.length !== undefined
              ? this.offsetToPosition(text, d.start + d.length)
              : undefined,
          text: this.ts.flattenDiagnosticMessageText(d.messageText, "\n"),
          code: d.code,
          category:
            d.category === this.ts.DiagnosticCategory.Error
              ? "error"
              : d.category === this.ts.DiagnosticCategory.Warning
                ? "warning"
                : "suggestion",
        }));
      }

      case "getCodeFixes": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const startPos = this.positionToOffset(
          text,
          args.startLine as number,
          args.startOffset as number,
        );
        const endPos = this.positionToOffset(
          text,
          args.endLine as number,
          args.endOffset as number,
        );
        const errorCodes = (args.errorCodes ?? []) as number[];

        const fixes = this.service.getCodeFixesAtPosition(
          file,
          startPos,
          endPos,
          errorCodes,
          {},
          {},
        );

        return fixes.map((fix) => ({
          description: fix.description,
          changes: fix.changes.map((change) => ({
            fileName: change.fileName,
            textChanges: change.textChanges.map((tc) => ({
              start: this.offsetToPosition(this.getFileText(change.fileName), tc.span.start),
              end: this.offsetToPosition(
                this.getFileText(change.fileName),
                tc.span.start + tc.span.length,
              ),
              newText: tc.newText,
            })),
          })),
        }));
      }

      case "encodedSemanticClassifications-full": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const startArg = args.start as { line: number; offset: number } | undefined;
        const endArg = args.end as { line: number; offset: number } | undefined;

        const startPos = startArg ? this.positionToOffset(text, startArg.line, startArg.offset) : 0;
        const endPos = endArg
          ? this.positionToOffset(text, endArg.line, endArg.offset)
          : text.length;

        const result = this.service.getEncodedSemanticClassifications(
          file,
          { start: startPos, length: endPos - startPos },
          "2020" as ts.SemanticClassificationFormat,
        );

        return { spans: result.spans };
      }

      case "documentHighlights": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const filesToSearch = (args.filesToSearch ?? [file]) as string[];
        const highlights = this.service.getDocumentHighlights(file, offset, filesToSearch);

        if (!highlights) return [];

        return highlights.map((group) => ({
          file: group.fileName,
          highlightSpans: group.highlightSpans.map((span) => ({
            start: this.offsetToPosition(this.getFileText(group.fileName), span.textSpan.start),
            end: this.offsetToPosition(
              this.getFileText(group.fileName),
              span.textSpan.start + span.textSpan.length,
            ),
            kind:
              span.kind === this.ts.HighlightSpanKind.writtenReference
                ? "writtenReference"
                : "reference",
          })),
        }));
      }

      case "provideInlayHints": {
        const file = args.file as string;
        const text = this.getFileText(file);

        const startPos = (args.start as number) ?? 0;
        const length = (args.length as number) ?? text.length;

        const hints = this.service.provideInlayHints(file, { start: startPos, length }, undefined);

        return hints.map((hint) => ({
          text: hint.text,
          position: this.offsetToPosition(text, hint.position),
          kind: hint.kind === this.ts.InlayHintKind.Type ? "Type" : "Parameter",
          whitespaceBefore: hint.whitespaceBefore,
          whitespaceAfter: hint.whitespaceAfter,
        }));
      }

      case "exit":
        return {};

      default:
        throw new Error(`Unknown command: ${command}`);
    }
  }

  // ── Helpers ─────────────────────────────────────────────────

  private getFileText(file: string): string {
    const snap = this.fileSnapshots.get(file);
    if (snap) return snap.getText(0, snap.getLength());
    try {
      return this.ts.sys.readFile(file) ?? "";
    } catch {
      return "";
    }
  }

  /** 1-based line/offset to 0-based byte offset */
  private positionToOffset(text: string, line: number, offset: number): number {
    let currentLine = 1;
    let i = 0;
    while (currentLine < line && i < text.length) {
      if (text[i] === "\n") currentLine++;
      i++;
    }
    return i + offset - 1;
  }

  /** 0-based byte offset to 1-based line/offset */
  private offsetToPosition(text: string, offset: number): { line: number; offset: number } {
    let line = 1;
    let lastLineStart = 0;
    for (let i = 0; i < offset && i < text.length; i++) {
      if (text[i] === "\n") {
        line++;
        lastLineStart = i + 1;
      }
    }
    return { line, offset: offset - lastLineStart + 1 };
  }

  private spanToRange(text: string, span: ts.TextSpan) {
    return {
      start: this.offsetToPosition(text, span.start),
      end: this.offsetToPosition(text, span.start + span.length),
    };
  }

  private groupBy<T>(items: T[], key: (item: T) => string): [string, T[]][] {
    const map = new Map<string, T[]>();
    for (const item of items) {
      const k = key(item);
      const arr = map.get(k) ?? [];
      arr.push(item);
      map.set(k, arr);
    }
    return [...map.entries()];
  }
}
