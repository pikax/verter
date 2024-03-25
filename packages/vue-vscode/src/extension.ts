import {
  window,
  commands,
  workspace,
  ExtensionContext,
  ProgressLocation,
  ViewColumn,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

import type { PatchClient } from "@verter/language-shared";
import { patchClient, NotificationType } from "@verter/language-shared";
import CompiledCodeContentProvider from "./CompiledCodeContentProvider";

type GetClient = () => PatchClient<LanguageClient>;

let getClient: GetClient | undefined;

export function activate(context: ExtensionContext) {
  const server = activateVueLanguageServer(context);
  getClient = server.getClient;

  server.getClient().sendNotification;
  server.getClient().onNotification;

  if (workspace.textDocuments.some((doc) => doc.languageId === "vue")) {
    commands.executeCommand(
      "_typescript.configurePlugin",
      "@verter/typescript-plugin",
      {
        enable: true,
      }
    );
  }
}

export function deactivate(): Thenable<void> | undefined {
  const stop = getClient?.().stop();
  getClient = undefined;
  return stop;
}

export function activateVueLanguageServer(context: ExtensionContext) {
  const runtimeConfig = workspace.getConfiguration("verter.language-server");

  const { workspaceFolders } = workspace;
  const rootPath = Array.isArray(workspaceFolders)
    ? workspaceFolders[0].uri.fsPath
    : undefined;

  const serverModule = require.resolve(
    "@verter/language-server/dist/server.js"
  );
  console.log("Using server from", serverModule);

  const runExecArgv: string[] = [];
  const port = runtimeConfig.get<number>("port") ?? -1;
  const debugArgs: string[] = [];

  if (port < 0) {
    debugArgs.push("--inspect=6009");
  } else {
    console.log("setting port to", port);
    runExecArgv.push(`--inspect=${port}`);
  }

  debugArgs.push(...runExecArgv);

  // If the extension is launched in debug mode then the debug server options are used
  // Otherwise the run options are used
  const serverOptions: ServerOptions = {
    run: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { execArgv: runExecArgv },
    },
    debug: {
      module: serverModule,
      transport: TransportKind.ipc,
      options: { execArgv: debugArgs },
    },
  };

  // Options to control the language client
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "vue" },
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "typescript" },
    ],
    // revealOutputChannelOn: RevealOutputChannelOn.Never,
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.{vue}"),
    },
    initializationOptions: {
      configuration: {
        vue: workspace.getConfiguration("vue"),
        prettier: workspace.getConfiguration("prettier"),
        emmet: workspace.getConfiguration("emmet"),
        typescript: workspace.getConfiguration("typescript"),
        javascript: workspace.getConfiguration("javascript"),
        css: workspace.getConfiguration("css"),
        less: workspace.getConfiguration("less"),
        scss: workspace.getConfiguration("scss"),
        html: workspace.getConfiguration("html"),
      },
      // dontFilterIncompleteCompletions: true,
    },
  };

  let client = createLanguageServer(serverOptions, clientOptions);
  const getClient = () => client as unknown as PatchClient<LanguageClient>;

  context.subscriptions.push(
    commands.registerCommand("verter.restartLanguageServer", async () => {
      await restartLS(true);
    })
  );

  let restarting = false;
  async function restartLS(showMsg: boolean) {
    if (restarting) {
      return;
    }
    restarting = true;
    try {
      await client.stop();
      client = createLanguageServer(serverOptions, clientOptions);
      await client.start();
      if (showMsg) {
        window.showInformationMessage("Verter Language server restarted");
      }
    } catch (e) {
      console.error(e);
    } finally {
      restarting = false;
    }
  }

  addDidChangeTextDocumentListener(getClient);
  addCompilePreviewCommand(getClient, context);

  return {
    getClient,
  };
}

function createLanguageServer(
  serverOptions: ServerOptions,
  clientOptions: LanguageClientOptions
) {
  return new LanguageClient("verter", "Verter", serverOptions, clientOptions);
}

function addDidChangeTextDocumentListener(getClient: GetClient) {
  workspace.onDidChangeTextDocument((e) => {
    if (
      e.document.languageId !== "typescript" &&
      e.document.languageId !== "javascript"
    ) {
      return;
    }
    const client = getClient();
    client.sendNotification(NotificationType.OnDidChangeTsOrJsFile, {
      uri: e.document.uri.toString(true),
      changes: e.contentChanges.map((x) => ({
        range: {
          start: {
            line: x.range.start.line,
            character: x.range.start.character,
          },
          end: { line: x.range.end.line, character: x.range.end.character },
        },
        text: x.text,
      })),
    });
  });
}

function addCompilePreviewCommand(
  getClient: GetClient,
  context: ExtensionContext
) {
  const compiledCodeContentProvider = new CompiledCodeContentProvider(
    getClient
  );

  context.subscriptions.push(
    // Register the content provider for "vue-compiled://" files
    workspace.registerTextDocumentContentProvider(
      CompiledCodeContentProvider.scheme,
      compiledCodeContentProvider
    ),
    compiledCodeContentProvider
  );

  context.subscriptions.push(
    commands.registerTextEditorCommand(
      "veter.showCompiledCodeToSide",
      async (editor) => {
        if (editor?.document?.languageId !== "vue") {
          window.showInformationMessage("Not a Vue file");
          return;
        }

        window.withProgress(
          { location: ProgressLocation.Window, title: "Compiling..." },
          async () => {
            // Open a new preview window for the compiled code
            return await window.showTextDocument(
              CompiledCodeContentProvider.previewWindowUri,
              {
                preview: true,
                viewColumn: ViewColumn.Beside,
                // TODO add selection to the window, it needs to be resolved
                // selection: editor.selection,
              }
            );
          }
        );
      }
    )
  );
}
