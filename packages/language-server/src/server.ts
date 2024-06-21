import { RequestType } from "@verter/language-shared";
import ts, { parseIsolatedEntityName } from "typescript";
import {
  Connection,
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
        definitionProvider: true,
        hoverProvider: true,
        workspace: {
          //   fileOperations: {},
          workspaceFolders: {
            supported: true,
            changeNotifications: true,
          },
        },
      },
    };
  });

  connection.onDefinition((params) => {
    console.log("on definition", params);

    return undefined;
  });

  connection.onHover((params) => {
    console.log("on hover", params);
    const uri = params.textDocument.uri;

    const tsService = manager.retrieveService(uri);
    if (!tsService) {
      return undefined;
    }

    let doc = documentManager.getDocument(uri);
    if (!doc) {
      return undefined;
    }

    let position = params.position;

    if (isVueDocument(doc)) {
      const subDoc = doc.getDocumentForPosition(params.position);
      if (subDoc) {
        doc = subDoc;
        position = subDoc.toGeneratedPosition(params.position);
      }
    } else {
    }
    try {
      const offset = doc.offsetAt(position);

      //   const semanatic = tsService.getSemanticDiagnostics(doc.uri);
      //   const definition = tsService.getTypeDefinitionAtPosition(doc.uri, offset);
      //   const references = tsService.getReferencesAtPosition(doc.uri, offset);
      //   const completion = tsService.getCompletionsAtPosition(doc.uri, offset, {
      //     triggerCharacter:
      //       params.context?.triggerCharacter === "@"
      //         ? ""
      //         : params.context?.triggerCharacter,

      //     includeSymbol: true,
      //     includeAutomaticOptionalChainCompletions: true,
      //     jsxAttributeCompletionStyle: "auto",
      //     importModuleSpecifierEnding: "auto",
      //     disableSuggestions: true,
      //   });

      const quickInfo = tsService.getQuickInfoAtPosition(doc.uri, offset);

      // const text = doc.getText();
      // const len = text.length;
      // for (let i = 50; i < len; i++) {
      //   const info = tsService.getQuickInfoAtPosition(doc.uri, i);

      //   console.log("info", i, text.slice(i - 1, i + 1), info);
      // }

      //   if (!quickInfo) {
      //     return undefined;
      //   }

      return undefined;
    } catch (e) {
      console.error("hover error", e);
      return undefined;
    }
  });

  connection.onRequest(RequestType.GetCompiledCode, (uri) => {
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
