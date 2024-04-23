import lsp, {
  CompletionItem,
  CompletionItemKind,
  DefinitionLink,
  DiagnosticTag,
  InsertTextFormat,
  LocationLink,
  Position,
  Range,
  TextDocuments,
} from "vscode-languageserver/node";
import {
  NotificationType,
  RequestType,
  patchClient,
} from "@verter/language-shared";
import logger from "./logger";
import { DocumentManager } from "./v2/lib/documents/manager";
import { VueDocument } from "./v2/lib/documents/document";
import {
  formatQuickInfo,
  getTypescriptService,
  itemToMarkdown,
  mapCompletion,
  mapDiagnostic,
} from "./v2/lib/plugins/typescript";
// import { uriToFilePath } from "vscode-languageserver/lib/node/files";

import { URI } from "vscode-uri";
import {
  isVerterVirtual,
  uriToVerterVirtual,
  urlToFileUri,
  urlToPath,
} from "./v2/lib/documents/utils";
import ts, {
  Diagnostic,
  DiagnosticWithLocation,
  GetCompletionsAtPositionOptions,
} from "typescript";

export interface LsConnectionOption {
  /**
   * The connection to use. If not provided, a new connection will be created.
   */
  connection?: lsp.Connection;

  logErrorsOnly?: boolean;
}

export function startServer(options: LsConnectionOption = {}) {
  let originalConnection = options.connection;
  if (!originalConnection) {
    if (process.argv.includes("--stdio")) {
      console.log = console.warn;
      originalConnection = lsp.createConnection(process.stdin, process.stdout);
    } else {
      originalConnection = lsp.createConnection(lsp.ProposedFeatures.all);
      // new lsp.IPCMessageReader(process),
      // new lsp.IPCMessageWriter(process)
      // );
    }
  }
  const connection = patchClient(originalConnection!);

  if (options?.logErrorsOnly !== undefined) {
    logger.setLogErrorsOnly(options.logErrorsOnly);
  }

  const documentManager = new DocumentManager();
  const tsService = getTypescriptService(process.cwd(), documentManager);

  connection.onInitialize((params) => {
    return {
      capabilities: {
        textDocumentSync: {
          openClose: true,
          change: lsp.TextDocumentSyncKind.Incremental,
          save: {
            includeText: false,
          },
        },
        // Tell the client that the server supports code completion
        completionProvider: {
          // allCommitCharacters: true,
          resolveProvider: true,
          completionItem: {
            labelDetailsSupport: true,
          },
          triggerCharacters: [".", "@", "<"],
        },
        referencesProvider: true,
        typeDefinitionProvider: true,
        hoverProvider: true,
        diagnosticProvider: true,
        declarationProvider: true,
        definitionProvider: true,
        // // inlayHintProvider: true,
        renameProvider: true,
      },
    };
  });

  connection.onRequest(RequestType.GetCompiledCode, (uri) => {
    const doc = documentManager.getDocument(uri);

    if (doc?.languageId === "vue") {
      const vueDoc = doc as VueDocument;

      return {
        js: {
          code: vueDoc.template.content,
          map: vueDoc.template.map,
        },
        css: {
          code: "",
          map: "",
        },
      };
    }

    return {
      js: {
        code: doc?.getText() ?? "",
        map: "",
      },
      css: {
        code: "",
        map: "",
      },
    };
  });

  connection.onHover((params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const doc = documentManager.getDocument<true>(params.textDocument.uri);
    if (!doc) {
      console.log("doc does not exist", params.textDocument.uri);
      return null;
    }

    const offset = doc.parsedOffsetFromPosition(params.position);
    const filepath = uriToVerterVirtual(params.textDocument.uri);
    const quickInfo = tsService.getQuickInfoAtPosition(filepath, offset);
    if (!quickInfo) {
      return null;
    }
    return formatQuickInfo(quickInfo, doc);
  });

  connection.onReferences((params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const doc = documentManager.getDocument<true>(params.textDocument.uri);
    if (!doc) {
      console.log("doc does not exist", params.textDocument.uri);
      return null;
    }

    const offset = doc.parsedOffsetFromPosition(params.position);
    const filepath = uriToVerterVirtual(params.textDocument.uri);
    const references = tsService.getReferencesAtPosition(filepath, offset);
    if (!references) {
      return null;
    }
    return references;
  });

  connection.onTypeDefinition((params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const doc = documentManager.getDocument<true>(params.textDocument.uri);
    if (!doc) {
      console.log("doc does not exist", params.textDocument.uri);
      return null;
    }

    const offset = doc.parsedOffsetFromPosition(params.position);
    const filepath = uriToVerterVirtual(params.textDocument.uri);
    const definition = tsService.getTypeDefinitionAtPosition(filepath, offset);
    if (!definition) {
      return null;
    }

    return definition.map((x) => {
      const doc = documentManager.getDocument(x.fileName);
      if (!doc || doc.languageId !== "vue") {
        const start = tsService.toLineColumnOffset!(
          x.fileName,
          x.textSpan.start
        );
        const end = tsService.toLineColumnOffset!(
          x.fileName,
          x.textSpan.start + x.textSpan.length
        );

        const contextStart = tsService.toLineColumnOffset!(
          x.fileName,
          x.contextSpan!.start
        );
        const contextEnd = tsService.toLineColumnOffset!(
          x.fileName,
          x.contextSpan!.start + x.contextSpan!.length
        );

        try {
          return LocationLink.create(
            x.fileName,
            Range.create(start, end),
            Range.create(contextStart, contextEnd)
          );
        } catch (e) {
          console.error(e);
          debugger;
          return x;
        }
      }
      const file = urlToFileUri(x.fileName)!;

      let contextSpan: Range | undefined = undefined;
      if (x.contextSpan) {
        const startPos = doc.originalPosition(x.contextSpan.start);
        const endPos = doc.originalPosition(
          x.contextSpan.start + x.contextSpan.length
        );
        try {
          contextSpan = Range.create(startPos, endPos);
        } catch (e) {
          console.error(e);
          debugger;
        }
      }

      try {
        const textSpan = Range.create(
          doc.originalPosition(x.textSpan.start),
          doc.originalPosition(x.textSpan.start + x.textSpan.length)
        );

        return LocationLink.create(file, contextSpan ?? textSpan, textSpan);
      } catch (e) {
        console.error(e);
        debugger;
      }

      return x;
    });
  });
  connection.onDefinition((params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const doc = documentManager.getDocument<true>(params.textDocument.uri);
    if (!doc) {
      console.log("doc does not exist", params.textDocument.uri);
      return null;
    }

    const offset = doc.parsedOffsetFromPosition(params.position);
    const filepath = uriToVerterVirtual(params.textDocument.uri);
    const definition = tsService.getDefinitionAtPosition(filepath, offset);
    if (!definition) {
      return null;
    }
    return definition.map((x) => {
      const doc = documentManager.getDocument(x.fileName);
      if (!doc || doc.languageId !== "vue") {
        const start = tsService.toLineColumnOffset!(
          x.fileName,
          x.textSpan.start
        );
        const end = tsService.toLineColumnOffset!(
          x.fileName,
          x.textSpan.start + x.textSpan.length
        );

        const contextStart = tsService.toLineColumnOffset!(
          x.fileName,
          x.contextSpan!.start
        );
        const contextEnd = tsService.toLineColumnOffset!(
          x.fileName,
          x.contextSpan!.start + x.contextSpan!.length
        );

        try {
          return LocationLink.create(
            x.fileName,
            Range.create(start, end),
            Range.create(contextStart, contextEnd)
          );
        } catch (e) {
          console.error(e);
          debugger;
          return x;
        }
      }
      const file = urlToFileUri(x.fileName)!;

      let contextSpan: Range | undefined = undefined;
      if (x.contextSpan) {
        const startPos = doc.originalPosition(x.contextSpan.start);
        const endPos = doc.originalPosition(
          x.contextSpan.start + x.contextSpan.length
        );
        try {
          contextSpan = Range.create(startPos, endPos);
        } catch (e) {
          console.error(e);
          debugger;
        }
      }

      try {
        const textSpan = Range.create(
          doc.originalPosition(x.textSpan.start),
          doc.originalPosition(x.textSpan.start + x.textSpan.length)
        );

        return LocationLink.create(file, contextSpan ?? textSpan, textSpan);
      } catch (e) {
        console.error(e);
        debugger;
      }

      return x;
    });
  });

  connection.onCompletion((params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;

    const document = documentManager.getDocument<true>(params.textDocument.uri);

    if (!document) return null;

    const offset = document.parsedOffsetFromPosition(params.position);

    try {
      const results = tsService.getCompletionsAtPosition(
        document._virtualUri,
        offset,
        {
          triggerKind: params.context?.triggerKind,
          triggerCharacter:
            params.context?.triggerCharacter === "@"
              ? ""
              : params.context?.triggerCharacter,

          includeSymbol: true,
          includeAutomaticOptionalChainCompletions: true,
          jsxAttributeCompletionStyle: "auto",
          importModuleSpecifierEnding: "auto",
          disableSuggestions: true,
        } as GetCompletionsAtPositionOptions
      );

      console.log("rrr", results);

      if (results) {
        return {
          isIncomplete: results.isIncomplete,
          items: results.entries.map((x) =>
            mapCompletion(x, {
              virtualUrl: document._virtualUri,
              index: offset,
              triggerKind: params.context?.triggerKind,
              triggerCharacter: params.context?.triggerCharacter,
            })
          ),
        };
      }

      return results;
    } catch (e) {
      console.error("eeee", e);
    }

    return {
      isIncomplete: false,
      items: [],
    };
  });

  connection.onCompletionResolve((item) => {
    const data: {
      virtualUrl: string;
      index: number;
      triggerKind: lsp.CompletionTriggerKind | undefined;
      triggerCharacter: string | undefined;
    } = item.data ?? {};

    const label = item.label.startsWith("@")
      ? `on${item.label[1].toLocaleUpperCase()}${item.label.slice(2)}`
      : item.label;

    const details = tsService.getCompletionEntryDetails(
      data.virtualUrl,
      data.index,
      label,
      undefined,
      undefined,
      undefined,
      undefined // item.data
    );
    if (!details) return item;

    let displayDetail = ts.displayPartsToString(details.displayParts) + "\n";
    item.detail = displayDetail;

    item.documentation = itemToMarkdown(details);

    return item;
  });

  connection.onRequest("textDocument/diagnostic", async (params) => {
    const doc = documentManager.getDocument(params.textDocument.uri);
    if (doc) {
      const diagnostics = await sendDiagnostics(doc);
      return diagnostics;
    }
    throw new Error(`No document found for URI ${params.textDocument.uri}`);
  });

  async function sendDiagnostics(document: VueDocument) {
    if (document.languageId !== "vue") return;

    try {
      const filename = document.virtualUri;

      let syntacticDiagnostics: DiagnosticWithLocation[] = [];
      try {
        syntacticDiagnostics = tsService.getSyntacticDiagnostics(filename);
      } catch (e) {
        console.error("syntacticDiagnostics", e);
      }
      let semanticDiagnostics: Diagnostic[] = [];
      try {
        semanticDiagnostics = tsService.getSemanticDiagnostics(filename);
      } catch (e) {
        console.error("semanticDiagnostics", e);
      }
      let suggestionDiagnostics: DiagnosticWithLocation[] = [];
      try {
        suggestionDiagnostics = tsService.getSuggestionDiagnostics(filename);
      } catch (e) {
        console.error("tsService.getSuggestionDiagnostics", e);
      }

      const allDiagnostics = [
        ...syntacticDiagnostics,
        ...semanticDiagnostics,
        ...suggestionDiagnostics,
      ]
        .map((x) => mapDiagnostic(x, document))
        .filter(Boolean);

      await connection.sendDiagnostics({
        uri: document.uri,
        diagnostics: allDiagnostics,
        version: document.version,
      });
      return allDiagnostics;
    } catch (e) {
      console.error(e);
      debugger;
    }
  }

  documentManager._textDocuments.onDidOpen((params) => {
    const doc = documentManager.getDocument(params.document.uri);

    if (!doc) return;
    if (doc.version !== params.document.version) {
      doc.overrideDoc(params.document);
    }
    sendDiagnostics(doc);
  });

  documentManager._textDocuments.onDidClose((params) => {
    const doc = documentManager.getDocument(params.document.uri);
    if (!doc) return;
    sendDiagnostics(doc);
  });

  documentManager.listen(originalConnection);
  connection.listen();
}

startServer();
