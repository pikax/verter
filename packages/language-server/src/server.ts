import {
  Connection,
  createConnection,
  ProposedFeatures,
  TextDocumentSyncKind,
} from "vscode-languageserver/node";
import { DocumentManager } from "./v5/documents/manager/manager";
import { VerterManager } from "./v5/documents/verter/manager";
import { isVueDocument, isVueFile, uriToPath } from "./v5/documents";
import { formatQuickInfo, mapDefinitionInfo } from "./v5/helpers";

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
        },
        renameProvider: true,
      },
    };
  });

  connection.onDefinition((params) => {
    console.log("doc", params.textDocument);
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
    const definitions = rrr
      .map((x) => {
        console.log("ff", x);

        return mapDefinitionInfo(x, documentManager, true);
      });

    console.log("def", definitions);

    return definitions;
  });

  connection.onHover((params) => {
    console.log("doc", params.textDocument);
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
    const quickInfo = docs
      .flatMap(({ doc, offset }) => {
        const qq = tsService.getQuickInfoAtPosition(doc.uri, offset);
        if (qq) {
          return formatQuickInfo(qq, doc);
        }
      })
      .find((x) => !!x);

    console.log("ddd", docs, quickInfo);

    return quickInfo;
  });

  connection.onRequest("$/getCompiledCode", async (params) => {
    return {
      js: {
        code: "code",
        map: "map",
      },
      css: {
        code: "code",
        map: "map",
      },
    };
  });

  documentManager.listen(connection);
  connection.listen();
}

startServer();
