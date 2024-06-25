import { RequestType } from "@verter/language-shared";
import ts, {
  parseIsolatedEntityName,
  type Diagnostic as TSDiagnostic,
  type DiagnosticWithLocation,
  type GetCompletionsAtPositionOptions,
} from "typescript";
import {
  CompletionTriggerKind,
  Connection,
  Diagnostic,
  LocationLink,
  ProposedFeatures,
  Range,
  TextDocumentSyncKind,
  createConnection,
} from "vscode-languageserver/node";
import { uriToPath } from "./v4/documents/utils.js";
import {
  VerterManager,
  DocumentManager,
  isVueDocument,
} from "./v4/documents/manager/index.js";

import fsPath from "path";
import { VueDocument } from "./v4/documents/index.js";
import { TextDocument } from "vscode-languageserver-textdocument";
import {
  formatQuickInfo,
  itemToMarkdown,
  mapCompletion,
  mapDiagnostic,
} from "./v4/helpers/typescript.js";

export interface LsConnectionOption {
  /**
   * The connection to use. If not provided, a new connection will be created.
   */
  connection?: Connection;

  logErrorsOnly?: boolean;
}

export function startServer(options: LsConnectionOption = {}) {
  let originalConnection = options.connection;
  if (!originalConnection) {
    if (process.argv.includes("--stdio")) {
      console.log = console.warn;
      originalConnection = createConnection(process.stdin, process.stdout);
    } else {
      originalConnection = createConnection(ProposedFeatures.all);
      // new lsp.IPCMessageReader(process),
      // new lsp.IPCMessageWriter(process)
      // );
    }
  }
  //   const connection = patchClient(originalConnection!);

  //   if (options?.logErrorsOnly !== undefined) {
  //     logger.setLogErrorsOnly(options.logErrorsOnly);
  //   }
  const connection = originalConnection;

  const documentManager = new DocumentManager();
  const manager = new VerterManager(documentManager);

  connection.onInitialize((params) => {
    manager.init(params);

    return {
      capabilities: {
        textDocumentSync: {
          openClose: true,
          change: TextDocumentSyncKind.Incremental,
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

        definitionProvider: true,
        hoverProvider: true,
        diagnosticProvider: {
          documentSelector: "*.vue",
          interFileDependencies: true,
          workspaceDiagnostics: false,
        },
        typeDefinitionProvider: true,
        declarationProvider: true,
        workspace: {
          //   fileOperations: {},
          workspaceFolders: {
            supported: true,
            changeNotifications: true,
          },
        },
        renameProvider: true,
      },
    };
  });

  connection.onDefinition((params) => {
    console.log("on definition", params);

    return undefined;
  });

  connection.onRenameRequest((params) => {
    const doc = documentManager.getDocument(params.textDocument.uri);
    if (!isVueDocument(doc)) {
      return undefined;
    }
    const subDoc = doc.getDocumentForPosition(params.position);
    if (!subDoc) {
      console.warn(
        "VERTER no document found for uri and position",
        params.textDocument.uri,
        params.position
      );
      return undefined;
    }

    subDoc.syncVersion();
    const offset = subDoc.toGeneratedOffsetFromPosition(params.position);
    const tsService = manager.getTsService(doc.uri);

    const locs = tsService.findRenameLocations(
      subDoc.uri,
      offset,
      false,
      false,
      {
        providePrefixAndSuffixTextForRename: true,
      }
    );

    if (!locs) {
      return undefined;
    }
    console.log("locs", locs.length);

    return locs.map((loc) => {
      const range = subDoc.toOriginalRange({
        start: subDoc.toOriginalPositionFromOffset(loc.textSpan.start),
        end: subDoc.toOriginalPositionFromOffset(
          loc.textSpan.start + loc.textSpan.length
        ),
      });

      const locDoc = documentManager.getDocument(loc.fileName);
      if (locDoc && isVueDocument(locDoc)) {
        return LocationLink.create(uriToPath(locDoc.uri), range, range, range);
      }

      return LocationLink.create(
        uriToPath(loc.fileName),
        range,
        // TODO fix this
        params.position as any
      );
    }) as any;
  });

  connection.onCompletion((params) => {
    const doc = documentManager.getDocument(params.textDocument.uri);
    if (!isVueDocument(doc)) {
      return undefined;
    }
    const subDoc = doc.getDocumentForPosition(params.position);
    if (!subDoc) {
      console.warn(
        "VERTER no document found for uri and position",
        params.textDocument.uri,
        params.position
      );
      return undefined;
    }

    subDoc.syncVersion();

    const offset = subDoc.toGeneratedOffsetFromPosition(params.position);
    const tsService = manager.getTsService(doc.uri);
    try {
      {
        const program = tsService.getProgram();
        const sourceFile = program?.getSourceFile(subDoc.uri);
        const fullText = sourceFile.getFullText();
        const text = sourceFile.getText();
        console.log(
          "starting completions",
          sourceFile?.fileName,
          sourceFile?.isDeclarationFile
        );
      }

      const results = tsService.getCompletionsAtPosition(subDoc.uri, offset, {
        triggerKind: params.context?.triggerKind,
        triggerCharacter:
          params.context?.triggerCharacter === "@"
            ? ""
            : params.context?.triggerCharacter,

        // includeSymbol: true,
        // includeAutomaticOptionalChainCompletions: true,
        // jsxAttributeCompletionStyle: "auto",
        // importModuleSpecifierEnding: "auto",
        // disableSuggestions: true,
      } as GetCompletionsAtPositionOptions);

      if (results) {
        return {
          isIncomplete: results.isIncomplete,
          items: results.entries.map((x) =>
            mapCompletion(x, {
              virtualUrl: subDoc.uri,
              index: offset,
              triggerKind: params.context?.triggerKind,
              triggerCharacter: params.context?.triggerCharacter,
            })
          ),
        };
      } else {
        const program = tsService.getProgram();
        const sourceFile = program?.getSourceFile(subDoc.uri);
        const fullText = sourceFile.getFullText();
        const text = sourceFile.getText();
        console.log(
          "no completions",
          sourceFile?.fileName,
          sourceFile?.isDeclarationFile
        );
      }

      return undefined;
    } catch (e) {
      console.error("completion error", e);
      return undefined;
    }
  });

  connection.onCompletionResolve((item) => {
    const data: {
      virtualUrl: string;
      index: number;
      triggerKind: CompletionTriggerKind | undefined;
      triggerCharacter: string | undefined;
    } = item.data ?? {};

    const tsService = manager.getTsService(data.virtualUrl);

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

  connection.onHover((params) => {
    console.log("on hover", params);
    const uri = params.textDocument.uri;

    const tsService = manager.getTsService(uri);
    if (!tsService) {
      return undefined;
    }

    let doc = documentManager.getDocument(uri);
    if (!doc) {
      return undefined;
    }

    if (!isVueDocument(doc)) {
      return undefined;
    }
    const subDoc = doc.getDocumentForPosition(params.position);
    try {
      const offset = subDoc.toGeneratedOffsetFromPosition(params.position);
      const quickInfo = tsService.getQuickInfoAtPosition(subDoc.uri, offset);

      if (!quickInfo) {
        return undefined;
      }
      return formatQuickInfo(quickInfo, subDoc);
    } catch (e) {
      console.error("hover error", e);
      return undefined;
    }
  });

  connection.onRequest("textDocument/diagnostic", async (params) => {
    const doc = documentManager.getDocument(params.textDocument.uri);
    if (doc) {
      const diagnostics = await sendDiagnostics(doc);
      return diagnostics;
    }
    // @ts-ignore
    throw new Error(`No document found for URI ${params.textDocument.uri}`);
  });

  documentManager.onDocumentOpen((doc) => {
    sendDiagnostics(doc);
  });
  const tsService = manager.getTsService(process.cwd());

  async function sendDiagnostics(document: VueDocument | TextDocument) {
    if (!isVueDocument(document)) return;

    console.time("sendDiagnostics");
    const diagnostics: Diagnostic[] = [];

    try {
      let lastSend = Date.now();
      for (const subDocument of Object.values(document.subDocuments)) {
        if (!subDocument.uri.endsWith(".bundle.ts")) {
          continue;
        }

        let syntacticDiagnostics: DiagnosticWithLocation[] = [];
        let semanticDiagnostics: TSDiagnostic[] = [];
        let suggestionDiagnostics: DiagnosticWithLocation[] = [];
        const virtualUri = subDocument.uri;

        try {
          syntacticDiagnostics = tsService.getSyntacticDiagnostics(
            subDocument.uri
          );
        } catch (e) {
          console.error("syntacticDiagnostics", e);
        }
        try {
          semanticDiagnostics = tsService.getSemanticDiagnostics(
            subDocument.uri
          );
        } catch (e) {
          console.error("semanticDiagnostics", e);
        }
        try {
          suggestionDiagnostics = tsService.getSuggestionDiagnostics(
            subDocument.uri
          );
        } catch (e) {
          console.error("tsService.getSuggestionDiagnostics", e);
        }

        const allDiagnostics = [
          ...syntacticDiagnostics,
          ...semanticDiagnostics,
          ...suggestionDiagnostics,
        ]
          .map((x) => mapDiagnostic(x, subDocument))
          .filter(Boolean);

        diagnostics.push(...allDiagnostics);
        connection.sendDiagnostics({
          uri: document.uri,
          diagnostics,
          version: document.version,
        });
        console.log(
          "sendDiagnostics",
          Date.now() - lastSend + "ms",
          diagnostics.length,
          subDocument.uri,
          document.version
        );

        if (diagnostics.length === 0) {
          const program = tsService.getProgram();
          const sourceFile = program?.getSourceFile(subDocument.uri);
          const fullText = sourceFile.getFullText();
          const text = sourceFile.getText();

          console.log(
            "no diagnostics",
            sourceFile?.fileName,
            sourceFile?.isDeclarationFile
          );
        }
        lastSend = Date.now();
      }

      return diagnostics;
    } catch (e) {
      console.error(e);
      debugger;
    } finally {
      console.timeEnd("sendDiagnostics");
    }
  }

  connection.onRequest(RequestType.GetCompiledCode, (uri) => {
    const doc = documentManager.getDocument(uri);
    if (doc) {
      if (isVueDocument(doc)) {
        const code = [
          doc.subDocuments.script.getText(),

          doc.subDocuments.template.getText(),
        ].join('\n\n\n//---\n\n')

        return {
          js: {
            code,
            map: "",
          },
          css: {
            code: "",
            map: "",
          },
        };
      }
    }

    return {
      js: {
        code: "",
        map: "",
      },
      css: {
        code: "",
        map: "",
      },
    };
  });

  documentManager.listen(connection);
  connection.listen();
}

startServer();
