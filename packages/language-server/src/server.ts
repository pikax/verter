import {
  CompletionItem,
  CompletionItemKind,
  CompletionTriggerKind,
  Connection,
  createConnection,
  DefinitionLink,
  InsertTextFormat,
  ProposedFeatures,
  TextDocumentSyncKind,
  WorkspaceFolder,
} from "vscode-languageserver/node";
import { DocumentManager } from "./v5/documents/manager/manager";
import { VerterManager } from "./v5/documents/verter/manager";
import {
  isVueDocument,
  isVueFile,
  isVueSubDocument,
  uriToPath,
  VerterDocument,
  VueDocument,
} from "./v5/documents";
import {
  formatQuickInfo,
  itemToMarkdown,
  mapCompletion,
  mapDefinitionInfo,
  mapDiagnostic,
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
import { MagicString } from "vue/compiler-sfc";
import { VueSubDocument } from "./v5/documents/verter/vue/sub/sub";
import {
  VueBundleDocument,
  VueStyleDocument,
} from "./v5/documents/verter/vue/sub";

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

  const patchedConnection = patchClient(connection);

  const documentManager = new DocumentManager();
  const verterManager = new VerterManager(documentManager);

  connection.onInitialize((params) => {
    verterManager.init(params);
    return {
      capabilities: {
        textDocumentSync: {
          openClose: true,
          change: TextDocumentSyncKind.Full,
          // change: TextDocumentSyncKind.Incremental,
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
          triggerCharacters: [".", "@", "<", ":", " "],
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
            // didCreate: { filters: ["*"] },
            // didDelete: { filters: ["*"] },
            didCreate: {
              filters: [{ pattern: { glob: "*" } }],
            },
            didDelete: [{ pattern: { glob: "*" } }],
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

    let definitions: Array<DefinitionLink & { key: string }> = [];

    // const rrr = docs.flatMap(
    //   (x) => tsService.getDefinitionAtPosition(x.doc.uri, x.offset) ?? []
    // );

    // const definitions = rrr.map((x) => {
    //   return mapDefinitionInfo(x, documentManager, true);
    // });

    for (const d of docs) {
      if (d.doc instanceof VueStyleDocument) {
        const r = d.doc.languageService.doValidation(d.doc, d.doc.stylesheet, {
          hover: {},
        });

        console.log("ddd", r);
      } else {
        for (const x of tsService.getDefinitionAtPosition(
          d.doc.uri,
          d.offset
        ) ?? []) {
          const r = mapDefinitionInfo(x, documentManager, true);
          if (!r) continue;
          definitions.push(r);
        }
      }
    }

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

          switch (doc.languageId) {
            case "css":
            case "scss":
            case "less":
            case "sass": {
              if (doc instanceof VueStyleDocument) {
                const pos = doc.toGeneratedPosition(params.position);

                return doc.languageService.doHover(doc, pos, doc.stylesheet);
              }
              break;
            }
            default: {
              const qq = tsService.getQuickInfoAtPosition(doc.uri, offset);
              console.log("quickinfo", performance.now() - ss, doc.uri);
              if (qq) {
                if (qq.kind) {
                  return formatQuickInfo(qq, doc);
                } else if (!anyResult) {
                  anyResult = formatQuickInfo(qq, doc);
                }
              }
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
    try {
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
          case "css":
          case "scss":
          case "less":
          case "sass": {
            // const s = d as unknown as VueStyleDocument;

            if (!(d.doc instanceof VueStyleDocument)) {
              console.error("not a valid style document");
              continue;
            }

            const pos = d.doc.toGeneratedPosition(params.position);

            const r = d.doc.languageService.doComplete(
              d.doc,
              pos,
              d.doc.stylesheet
            );

            console.log("resoilved", r);

            items.push(...r.items);

            break;
          }
          case "tsx":
          case "ts":
          case "js":
          case "jsx":
            const tsService = verterManager.getTsService(d.doc.uri);
            if (!tsService) {
              continue;
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
                includeAutomaticOptionalChainCompletions: true,
                jsxAttributeCompletionStyle: "auto",
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

      const cssProp = items.find(
        (x) => x.kind === CompletionItemKind.Function && x.label === "var()"
      );
      if (cssProp || true) {
        items.push({
          ...(cssProp ?? {}),
          label: "[WIP]v-bind() ",
          kind: CompletionItemKind.Function,
          insertTextFormat: InsertTextFormat.Snippet, // Use snippet format for dynamic arguments
          insertText: "v-bind(${1|foo,bar,baz|})", // Add options as a snippet
          textEdit: cssProp?.textEdit
            ? "insert" in cssProp?.textEdit
              ? {
                  newText: "v-bind(${1|foo,bar,baz|})", // Ensure the snippet matches the insert text
                  insert: cssProp?.textEdit.insert,
                  replace: cssProp?.textEdit.replace,
                }
              : cssProp?.textEdit
            : undefined,
          documentation: {
            kind: "markdown",
            value:
              "A Vue directive for binding dynamic values.\n\nArguments:\n- `foo`: Description of foo.\n- `bar`: Description of bar.\n- `baz`: Description of baz.",
          },
        });
      }

      return {
        isIncomplete: false,
        items,
      };
    } catch (e) {
      console.error("failed onCompletion", e);
    }
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

  connection.onRequest("textDocument/diagnostic", async (params) => {
    const document = documentManager.getDocument(params.textDocument.uri);
    if (!document || !isVueDocument(document)) {
      return;
    }
    sendDiagnostics(document);
  });

  connection.onDidOpenTextDocument((params) => {
    const doc = documentManager.getDocument(params.textDocument.uri);
    if (!doc) {
      return;
    }
    if (isVueDocument(doc)) {
      sendDiagnostics(doc);
    }
  });

  function sendDiagnostics(document: VueDocument) {
    if (!isVueDocument(document)) return;

    console.time("sendDiagnostics");
    const tsService = verterManager.getTsService(document.uri);
    if (!tsService) {
      return;
    }

    function getDiagnostics(doc: VueSubDocument) {
      if (doc instanceof VueStyleDocument) {
        return doc.languageService.doValidation(doc, doc.stylesheet, {
          validate: true,
        });
      }

      const diagnostics = [
        ...tsService.getSemanticDiagnostics(doc.uri),
        ...tsService.getSyntacticDiagnostics(doc.uri),
        ...tsService.getSuggestionDiagnostics(doc.uri),
      ];
      return diagnostics.map((x) => mapDiagnostic(x, doc));
    }

    const diagnostics = document.docs
      .filter(
        (x) =>
          (x.languageId === "ts" || x.languageId === "tsx") &&
          !(x instanceof VueBundleDocument)
      )
      .flatMap((x) => {
        return getDiagnostics(x);
      })
      .filter((x) => !!x);

    connection.sendDiagnostics({
      uri: document.uri,
      diagnostics,
      version: document.version,
    });
  }

  // connection.onRequest("$/getCompiledCode", async (params) => {
  patchedConnection.onRequest(RequestType.GetCompiledCode, async (params) => {
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
//# sourceMappingURL=${x.sourceMapURL()}

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

  // patchedConnection.onNotification(
  //   NotificationType.OnFileChanged,
  //   async (params) => {
  //     params.type;
  //     params.uri;

  //     // TODO check why this is called here very odd
  //     if ("changes" in params) {
  //       console.log("applyed partial change", params);
  //       // documentManager.applyFileChanges(params.uri, params.changes as any);
  //     } else {
  //       console.log("file changed", params);
  //       documentManager.handleFileChange(params.uri, params.type);
  //     }

  //     // documentManager.handleFileChange(params.uri, params.type);
  //   }
  // );

  // patchedConnection.onNotification(
  //   NotificationType.OnDidChangeTsOrJsFile,
  //   async (params) => {
  //     params.changes;
  //     params.uri;

  //     documentManager.applyFileChanges(params.uri, params.changes);
  //     console.log("file changedXX", params);
  //   }
  // );

  documentManager.listen(connection);
  connection.listen();
}

startServer();
