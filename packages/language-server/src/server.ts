// import lsp from "vscode-languageserver/lib/node/main.js";
import lsp, { TextDocuments } from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import Logger from "./logger";

import { readFile } from "fs/promises";

import { NotificationType, patchClient } from "@verter/language-shared";
import { RequestType } from "@verter/language-shared";

import { createBuilder } from "@verter/core";
import { documentManager } from "./lib/documents/Manager";
import { getTypescriptService } from "./plugins/typescript/TypescriptPlugin";
import { offsetAt } from "./lib/documents/utils";
import { VueDocument } from "./lib/documents/VueDocument";

// import {
//   ClientCapabilities,
//   DocumentContext,
//   getDefaultHTMLDataProvider,
//   getLanguageService,
//   HTMLDocument,
//   IAttributeData,
//   IHTMLDataProvider,
//   LanguageService,
//   newHTMLDataProvider,
//   Scanner,
//   TokenType,
// } from 'vscode-html-languageservice'



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
  });

  connection.onInitialize((params) => {
    // console.log("inited --- lOL");
    // debugger;

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
          resolveProvider: true,
          triggerCharacters: [".", "@", "<"],
        },
        // typeDefinitionProvider: true,
        // hoverProvider: true,
      },
    };
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

  const tsService = getTypescriptService(process.cwd());



  // connection.client.connection


  connection.onCompletion(async (params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    let doc = documentManager.getDocument(params.textDocument.uri);

    if (doc) {
      {
        // const virtualUrl = params.textDocument.uri /*params.textDocument.uri.replace('file:', 'virtual:') +*/ + '.d.tsx'
        const virtualUrl = params.textDocument.uri.replace('.vue', '.ts')

        let newIndex = 8

        try {
          const results = tsService.getCompletionsAtPosition(virtualUrl, newIndex, {
            ...params.context,
          })
          console.log('got res', results?.entries.length)
          // return results?.entries ?? []
          if (results) {
            return {
              isIncomplete: true,
              items: results.entries.map(x => {
                return {
                  label: x.name,
                  kind: x.kind,
                  data: x.data
                }
              })
            }
          }
        } catch (e) {
          console.error('eeee', e)
        }
      }




      const content = doc._content ?? doc.content;

      doc = new VueDocument(doc.uri, content)
      const templateInfo = doc.template
      // const originalOffset = doc.offsetAt(params.position)

      const originalIndex = offsetAt(params.position, content)

      const templatePos = templateInfo.mapConsumer.generatedPositionFor({
        column: params.position.character,
        line: params.position.line + 1,
        source: '.'

      })

      console.log('getting pos', originalIndex)

      const index = offsetAt({
        character: templatePos.column,
        line: templatePos.line - 1
      }, templateInfo.content)




      // const virtualUrl = params.textDocument.uri /*params.textDocument.uri.replace('file:', 'virtual:') +*/ + '.d.tsx'
      const virtualUrl = params.textDocument.uri.replace('.vue', '.ts')

      let newIndex = 5

      try {
        const results = tsService.getCompletionsAtPosition(virtualUrl, newIndex, {
          ...params.context,
        })
        console.log('got res', results?.entries.length)
        // return results?.entries ?? []
        if (results) {
          return {
            isIncomplete: true,
            items: results.entries.map(x => {
              return {
                label: x.name,
                kind: x.kind,
                data: x.data
              }
            })
          }
        }
      } catch (e) {
        console.error('eeee', e)
      }

      return {

        isIncomplete: true,
        items: [
          {
            label: "hello",
            kind: lsp.CompletionItemKind.Class,
            data: 1,
          },
        ],
      };
    }



    // const r = tsService.getCompletionsAtPosition(
    //   decodeURIComponent(
    //     params.textDocument.uri
    //       .replace("Test.vue", "Test.my.ts")
    //       .replace("file:///", "")
    //   ),
    //   params.position,
    //   { triggerKind: 1, triggerCharacter: "." }
    // );

    // debugger;
    // return r;

    return {
      isIncomplete: false,
      items: [
        {
          label: "hello",
          kind: lsp.CompletionItemKind.Class,
          data: 1,
        },
      ],
    };
  });

  console.log("should be listening now...");

  documentManager.listen(connection);

  connection.listen();
}
startServer();
