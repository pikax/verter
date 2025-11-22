import * as cp from "child_process";
import * as path from "path";
import {
  createProtocolConnection,
  ProtocolConnection,
} from "vscode-languageserver-protocol";
import { IPCMessageReader, IPCMessageWriter } from "vscode-jsonrpc/node";
import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";

export const testWorkspacePath = path.resolve(
  __dirname,
  "../../test-workspace"
);

let vueServerProcess: cp.ChildProcess | undefined;
let tsserverProcess: cp.ChildProcess | undefined;
let connection: ProtocolConnection | undefined;
let seq = 0;

const requestHandlers = new Map<
  number,
  { resolve: (res: any) => void; reject: (err: any) => void }
>();

export async function getLanguageServer() {
  if (!connection) {
    // 1. Start TSServer
    const tsserverPath = path.join(
      __dirname,
      "..",
      "..",
      "node_modules",
      "typescript",
      "lib",
      "tsserver.js"
    );
    tsserverProcess = cp.fork(
      tsserverPath,
      [
        "--useNodeIpc",
        "--disableAutomaticTypingAcquisition",
        "--globalPlugins",
        "@vue/typescript-plugin",
        "--suppressDiagnosticEvents",
      ],
      {
        stdio: "inherit",
        execArgv: [], // Ensure we don't inherit debug flags that might conflict
      }
    );

    // Handle TSServer responses
    tsserverProcess.on("message", (msg: any) => {
      if (msg.type === "response") {
        const handler = requestHandlers.get(msg.request_seq);
        if (handler) {
          requestHandlers.delete(msg.request_seq);
          if (msg.success) {
            handler.resolve(msg);
          } else {
            handler.reject(msg);
          }
        }
      } else if (msg.type === "event") {
        // console.log('TSServer event:', msg.event);
      }
    });

    // 2. Start Vue Language Server
    const vueServerPath = require.resolve("@vue/language-server");
    vueServerProcess = cp.fork(vueServerPath, ["--node-ipc"], {
      cwd: testWorkspacePath,
      execArgv: [],
    });

    connection = createProtocolConnection(
      new IPCMessageReader(vueServerProcess),
      new IPCMessageWriter(vueServerProcess)
    );

    connection.listen();

    // 3. Handle tsserver/request from Vue LS
    connection.onNotification(
      "tsserver/request",
      async ([id, command, args]: [string, string, any]) => {
        const currentSeq = seq++;

        // Create a promise to wait for TSServer response
        const responsePromise = new Promise((resolve, reject) => {
          requestHandlers.set(currentSeq, { resolve, reject });
        });

        // Send to TSServer
        tsserverProcess?.send({
          seq: currentSeq,
          type: "request",
          command,
          arguments: args,
        });

        try {
          const res: any = await responsePromise;
          connection?.sendNotification("tsserver/response", [id, res.body]);
        } catch (err) {
          connection?.sendNotification("tsserver/response", [id, undefined]);
        }
      }
    );

    // 4. Initialize Vue LS
    await connection.sendRequest("initialize", {
      processId: process.pid,
      rootUri: URI.file(testWorkspacePath).toString(),
      capabilities: {
        textDocument: {
          completion: {
            completionItem: {
              snippetSupport: true,
              labelDetailsSupport: true,
            },
          },
          hover: {
            contentFormat: ["markdown", "plaintext"],
          },
        },
        workspace: {
          workspaceFolders: true,
          configuration: true,
        },
      },
      workspaceFolders: [
        {
          uri: URI.file(testWorkspacePath).toString(),
          name: "test-workspace",
        },
      ],
      initializationOptions: {
        typescript: {
          tsdk: path.join(
            __dirname,
            "..",
            "..",
            "node_modules",
            "typescript",
            "lib"
          ),
        },
      },
    });

    await connection.sendNotification("initialized", {});

    // Handle configuration requests (similar to volar-server.ts)
    connection.onRequest("workspace/configuration", ({ items }: any) => {
      return items.map((item: any) => {
        if (item.section?.startsWith("vue.inlayHints.")) {
          return true;
        }
        return null;
      });
    });
  }

  return {
    vueserver: {
      connection,
      // Mocking what tests expect from LanguageServerHandle
      openInMemoryDocument: async (
        uri: string,
        languageId: string,
        content: string
      ) => {
        await connection!.sendNotification("textDocument/didOpen", {
          textDocument: {
            uri,
            languageId,
            version: 1,
            text: content,
          },
        });
        return TextDocument.create(uri, languageId, 1, content);
      },
      closeTextDocument: async (uri: string) => {
        await connection!.sendNotification("textDocument/didClose", {
          textDocument: { uri },
        });
      },
      sendCompletionRequest: async (uri: string, position: any) => {
        return await connection!.sendRequest("textDocument/completion", {
          textDocument: { uri },
          position,
        });
      },
    },
    tsserver: {
      message: async (msg: any) => {
        // If the test provides a seq, use it, otherwise generate one
        const currentSeq = msg.seq ?? seq++;

        const responsePromise = new Promise((resolve, reject) => {
          requestHandlers.set(currentSeq, { resolve, reject });
        });

        tsserverProcess?.send({
          ...msg,
          seq: currentSeq,
          type: "request",
        });

        try {
          const res: any = await responsePromise;
          return { success: true, body: res.body };
        } catch (err: any) {
          return { success: false, body: err.message };
        }
      },
    },
    nextSeq: () => seq++,
    open: async (uri: string, languageId: string, content: string) => {
      // Update TSServer
      const currentSeq = seq++;
      const responsePromise = new Promise((resolve, reject) => {
        requestHandlers.set(currentSeq, { resolve, reject });
      });

      tsserverProcess?.send({
        seq: currentSeq,
        type: "request",
        command: "updateOpen",
        arguments: {
          changedFiles: [],
          closedFiles: [],
          openFiles: [
            {
              file: URI.parse(uri).fsPath,
              fileContent: content,
            },
          ],
        },
      });

      await responsePromise;

      // Send didOpen to Vue LS
      await connection!.sendNotification("textDocument/didOpen", {
        textDocument: {
          uri,
          languageId,
          version: 1,
          text: content,
        },
      });

      return TextDocument.create(uri, languageId, 1, content);
    },
    close: async (uri: string) => {
      const currentSeq = seq++;
      const responsePromise = new Promise((resolve, reject) => {
        requestHandlers.set(currentSeq, { resolve, reject });
      });

      tsserverProcess?.send({
        seq: currentSeq,
        type: "request",
        command: "close",
        arguments: {
          file: URI.parse(uri).fsPath,
        },
      });

      await responsePromise;

      // Send didClose to Vue LS
      await connection!.sendNotification("textDocument/didClose", {
        textDocument: { uri },
      });
    },
  };
}

export async function closeLanguageServer() {
  if (connection) {
    try {
      await connection.sendRequest("shutdown", {});
      await connection.sendNotification("exit", {});
    } catch (e) {
      // ignore
    }
    connection.dispose();
    connection = undefined;
  }

  if (vueServerProcess) {
    vueServerProcess.kill();
    vueServerProcess = undefined;
  }

  if (tsserverProcess) {
    tsserverProcess.kill();
    tsserverProcess = undefined;
  }
}
