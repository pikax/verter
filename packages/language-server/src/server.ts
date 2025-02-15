import {
  CompletionItem,
  CompletionTriggerKind,
  Connection,
  createConnection,
  ProposedFeatures,
  TextDocumentSyncKind,
} from "vscode-languageserver/node";
import { DocumentManager } from "./v5/documents/manager/manager";
import { VerterManager } from "./v5/documents/verter/manager";
import { isVueDocument, isVueFile, uriToPath } from "./v5/documents";
import {
  formatQuickInfo,
  itemToMarkdown,
  mapCompletion,
  mapDefinitionInfo,
} from "./v5/helpers";
import ts, {
  CompletionsTriggerCharacter,
  parseCommandLine,
  QuickInfo,
} from "typescript";

import {
  NotificationType,
  patchClient,
  RequestType,
} from "@verter/language-shared";

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
  const connection = patchClient(originalConnection);

  const documentManager = new DocumentManager();
  const verterManager = new VerterManager(documentManager);
  connection.onInitialize((params) => {
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
        referencesProvider: true,
        typeDefinitionProvider: true,
        declarationProvider: true,
        workspace: {
          //   fileOperations: {},
          workspaceFolders: {
            supported: true,
            changeNotifications: true,
          },
          fileOperations: {
            didCreate: { filters: ["*"] },
            didDelete: { filters: ["*"] },
          },
        },
        renameProvider: true,
      },
    };
  });

  connection.onDefinition((params) => {
    console.log("defintiona doc", params.textDocument);
    const uri = params.textDocument.uri;

    const doc = documentManager.getDocument(uri);
    if (!doc || !isVueDocument(doc)) {
      return undefined;
    }
    const tsService = verterManager.getTsService(uri);

    if (!tsService) {
      return undefined;
    }

    const docs = doc.docsForPos(params.position);

    const rrr = docs.flatMap(
      (x) => tsService.getDefinitionAtPosition(x.doc.uri, x.offset) ?? []
    );

    // const definitions = docs
    //   .flatMap(
    //     (x) => tsService.getDefinitionAtPosition(x.doc.uri, x.offset) ?? []
    //   )
    const definitions = rrr.map((x) => {
      return mapDefinitionInfo(x, documentManager, true);
    });

    return definitions;
  });

  connection.onHover((params) => {
    const start = performance.now();
    console.log("doc hover", params.textDocument);
    const uri = params.textDocument.uri;

    const doc = documentManager.getDocument(uri);
    if (!doc || !isVueDocument(doc)) {
      return undefined;
    }
    const tsService = verterManager.getTsService(uri);
    if (!tsService) {
      return undefined;
    }

    const docs = doc.docsForPos(params.position);

    try {
      let anyResult = undefined as
        | ReturnType<typeof formatQuickInfo>
        | undefined;
      const quickInfo = docs
        .flatMap(({ doc, offset }) => {
          const ss = performance.now();
          const qq = tsService.getQuickInfoAtPosition(doc.uri, offset);
          console.log("quickinfo", performance.now() - ss, doc.uri);
          if (qq) {
            if (qq.kind) {
              return formatQuickInfo(qq, doc);
            } else if (!anyResult) {
              anyResult = formatQuickInfo(qq, doc);
            }
          }
        })
        .find((x) => !!x);

      console.log("completed at", performance.now() - start);
      return quickInfo ?? anyResult;
    } catch (e) {
      console.error("hover", e);
      return undefined;
    }
  });

  connection.onCompletion((params) => {
    console.log("oncompletion doc", params.textDocument);
    const uri = params.textDocument.uri;

    const doc = documentManager.getDocument(uri);
    if (!doc || !isVueDocument(doc)) {
      return undefined;
    }

    const subDocs = doc.docsForPos(params.position);

    const items = [] as CompletionItem[];

    for (const d of subDocs) {
      const offset = d.offset;

      switch (d.doc.languageId) {
        case "tsx":
        case "ts":
        case "js":
        case "jsx":
          const tsService = verterManager.getTsService(d.doc.uri);
          if (!tsService) {
            return undefined;
          }
          const completions = tsService.getCompletionsAtPosition(
            d.doc.uri,
            offset,
            {
              triggerKind: params.context?.triggerKind,
              triggerCharacter:
                params.context?.triggerCharacter === "@"
                  ? " "
                  : (params.context
                      ?.triggerCharacter as CompletionsTriggerCharacter),

              // includeSymbol: true,
              // includeAutomaticOptionalChainCompletions: true,
              // jsxAttributeCompletionStyle: "auto",
              // importModuleSpecifierEnding: "auto",
              // disableSuggestions: true,
              // allowIncompleteCompletions: true,
            }
          );
          if (completions) {
            items.push(
              ...completions.entries
                // TODO improve filter
                .filter((x) => !x.name.startsWith("___VERTER___"))
                .map((x) =>
                  mapCompletion(x, {
                    virtualUrl: d.doc.uri,
                    index: offset,
                    triggerKind: params.context?.triggerKind,
                    triggerCharacter: params.context?.triggerCharacter,
                  })
                )
            );
          }
      }
    }

    console.log("found docs", subDocs);

    return {
      isIncomplete: false,
      items,
    };
  });

  connection.onCompletionResolve((item) => {
    if (!item.data) {
      return item;
    }
    const data: {
      virtualUrl: string;
      index: number;
      triggerKind: CompletionTriggerKind | undefined;
      triggerCharacter: string | undefined;
    } = item.data ?? {};

    const tsService = verterManager.getTsService(data.virtualUrl);

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

  // connection.onRequest("$/getCompiledCode", async (params) => {
  connection.onRequest(RequestType.GetCompiledCode, async (params) => {
    const doc = documentManager.getDocument(params);

    if (!doc || !isVueDocument(doc)) {
      return null;
    }

    return {
      js: {
        code: doc.docs
          .map(
            (x) => `// start ${x.uri}\n
${x.getText()}

//end ${x.uri}\n`
          )
          .join("\n//-----\n"),
        map: "map",
      },
      css: {
        code: "code",
        map: "map",
      },
    };
  });

  connection.onNotification(NotificationType.OnFileChanged, async (params) => {
    params.type;
    params.uri;

    documentManager.handleFileChange(params.uri, params.type);
    console.log("file changed", params);
  });

  documentManager.listen(connection as unknown as Connection);
  connection.listen();
}

startServer();
