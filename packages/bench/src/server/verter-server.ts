import {
  createConnection,
  IPCMessageReader,
  IPCMessageWriter,
} from "vscode-languageserver/node";
import type { Connection } from "vscode-languageserver";
import * as path from "node:path";
import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";
import { fork, type ChildProcess } from "node:child_process";

let connection: Connection | undefined;
let childProcess: ChildProcess | undefined;
let documents: Map<string, TextDocument> = new Map();

export const testWorkspacePath = path.resolve(
  __dirname,
  "../../test-workspace"
);
const serverPath = path.resolve(
  __dirname,
  "../../../language-server/dist/server.js"
);

export interface VerterServer {
  connection: Connection;
  openDocument: (
    uri: string,
    languageId: string,
    content: string
  ) => Promise<TextDocument>;
  closeDocument: (uri: string) => Promise<void>;
  getCompletions: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
  getHover: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
  getDefinition: (
    uri: string,
    position: { line: number; character: number }
  ) => Promise<any>;
}

export async function getVerterServer(): Promise<VerterServer> {
  if (!connection) {
    // Spawn the server process
    childProcess = fork(serverPath, ["--node-ipc"], {
      cwd: testWorkspacePath,
      // silent: true // Uncomment to suppress server output
    });

    // Create client connection using IPC
    const clientConnection = createConnection(
      new IPCMessageReader(childProcess),
      new IPCMessageWriter(childProcess)
    );

    clientConnection.listen();

    // Initialize the server
    await clientConnection.sendRequest("initialize", {
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
    });

    await clientConnection.sendNotification("initialized", {});
    // Give the server a moment to fully initialize
    await Promise.race([
      new Promise<void>((resolve) => {
        clientConnection.onNotification("serverReady", () => {
          console.log("Verter server initialized");
          resolve();
        });
      }),
      new Promise((resolve) => setTimeout(resolve, 5000)), // Timeout after 5 seconds
    ]);

    // Replace the connection with clientConnection for sending requests
    connection = clientConnection;
  }

  return {
    connection,
    openDocument: async (uri: string, languageId: string, content: string) => {
      const document = TextDocument.create(uri, languageId, 1, content);
      documents.set(uri, document);

      await connection!.sendNotification("textDocument/didOpen", {
        textDocument: {
          uri,
          languageId,
          version: document.version,
          text: content,
        },
      });

      // Some server flows rely on an explicit didChange after open to populate caches
      await connection!.sendNotification("textDocument/didChange", {
        textDocument: { uri, version: document.version + 1 },
        contentChanges: [{ text: content }],
      });

      return TextDocument.create(
        uri,
        languageId,
        document.version + 1,
        content
      );
    },
    closeDocument: async (uri: string) => {
      documents.delete(uri);
      await connection!.sendNotification("textDocument/didClose", {
        textDocument: { uri },
      });
    },
    getCompletions: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      return await connection!.sendRequest("textDocument/completion", {
        textDocument: { uri },
        position,
      });
    },
    getHover: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      return await connection!.sendRequest("textDocument/hover", {
        textDocument: { uri },
        position,
      });
    },
    getDefinition: async (
      uri: string,
      position: { line: number; character: number }
    ) => {
      return await connection!.sendRequest("textDocument/definition", {
        textDocument: { uri },
        position,
      });
    },
  };
}

export async function closeVerterServer() {
  if (connection) {
    try {
      await connection.sendRequest("shutdown", {});
      await connection.sendNotification("exit", {});
    } catch (e) {
      // Ignore errors during shutdown
    }
    connection.dispose();
    connection = undefined;
    documents.clear();
  }

  if (childProcess) {
    childProcess.kill();
    childProcess = undefined;
  }
}
