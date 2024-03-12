// import lsp from "vscode-languageserver/lib/node/main.js";
import lsp, { TextDocuments } from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import Logger from "./logger";

import { readFile } from "fs/promises";

import { NotificationType, patchClient } from "@verter/language-shared";
import { RequestType } from "@verter/language-shared";

import { createBuilder } from "@verter/core";

export interface LsConnectionOption {
  /**
   * The connection to use. If not provided, a new connection will be created.
   */
  connection?: lsp.Connection;

  logErrorsOnly?: boolean;
}

export function startServer(options: LsConnectionOption = {}) {
  Logger.error("in startServer", options);
  //   const connection = lsp.createConnection(lsp.ProposedFeatures.all);

  let connection = patchClient(options.connection);
  if (!connection) {
    if (process.argv.includes("--stdio")) {
      console.log = console.warn;
      connection = patchClient(
        lsp.createConnection(process.stdin, process.stdout)
      );
    } else {
      connection = patchClient(lsp.createConnection(lsp.ProposedFeatures.all));
      // new lsp.IPCMessageReader(process),
      // new lsp.IPCMessageWriter(process)
      // );
    }
  }

  if (options?.logErrorsOnly !== undefined) {
    Logger.setLogErrorsOnly(options.logErrorsOnly);
  }

  connection.onInitialized(() => {
    console.log("inited XXX--- lOL");
    debugger;
  });

  connection.onInitialize((e) => {
    console.log("inited --- lOL");
    debugger;
  });

  connection.onNotification(NotificationType.OnDidChangeTsOrJsFile, (e) => {
    // TODO add/update the file

    console.log("got notification ", NotificationType.OnDidChangeTsOrJsFile, e);
  });
  const builder = createBuilder();

  connection.onRequest(RequestType.GetCompiledCode, async (uri) => {
    console.log(
      "got request ",
      RequestType.GetCompiledCode,
      uri,
      decodeURIComponent(uri)
    );

    const uriPath = decodeURIComponent(uri).replace("file:///", "");
    console.log("opening ", uriPath);

    const content = await readFile(uriPath, "utf-8");

    const result = await builder.process(uriPath, content);

    console.log("resovled request ", uriPath, result.length);

    //todo handle doc
    return {
      js: {
        code: result,
        map: "",
      },
      css: {
        code: "",
        map: "",
      },
    };
  });

  console.log("should be listening now...");
  connection.listen();
}
startServer();
