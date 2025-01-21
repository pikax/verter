import {
  Connection,
  createConnection,
  ProposedFeatures,
  TextDocumentSyncKind,
} from "vscode-languageserver/node";
import { DocumentManager } from "./v5/documents/manager/manager";

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

startServer()
