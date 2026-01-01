import {
  window,
  commands,
  workspace,
  ExtensionContext,
  ProgressLocation,
  ViewColumn,
  FileSystemWatcher,
  WorkspaceFolder,
  OutputChannel,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

import { join, normalize } from "path";

import type { PatchClient } from "@verter/language-shared";
import {
  patchClient,
  NotificationType,
  RequestType,
} from "@verter/language-shared";
import type {
  StatisticsSnapshot,
  StatisticsSummary,
} from "@verter/language-shared";
import CompiledCodeContentProvider from "./CompiledCodeContentProvider";

import { resolveAndDownloadBinding } from "@verter/oxc-bindings";

type GetClient = () => PatchClient<LanguageClient>;

let getClient: GetClient | undefined;

console.log("hello tghere");

export async function activate(context: ExtensionContext) {
  console.log("activate", __dirname, __filename, context.extensionPath);

  await resolveAndDownloadBinding(context.extensionPath);
  const server = activateVueLanguageServer(context);
  getClient = server.getClient;

  server.getClient().sendNotification;
  server.getClient().onNotification;

  if (workspace.textDocuments.some((doc) => doc.languageId === "vue")) {
    commands.executeCommand(
      "_typescript.configurePlugin",
      require.resolve("@verter/typescript-plugin"),
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
  console.log("activateVueLanguageServer");
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
      statistics: getStatisticsInitialization(rootPath),
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

  addWriteVirtualFilesCommand(getClient, context);

  addShowStatisticsCommand(getClient, context);

  addNodeModulesChangedListener(getClient);

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

function getStatisticsInitialization(rootPath: string | undefined) {
  const config = workspace.getConfiguration("verter.statistics");
  const persistToFile = config.get<boolean>("persistToFile") ?? false;
  const configuredPath = config.get<string>("filePath") || undefined;

  const defaultPath =
    persistToFile && rootPath
      ? join(rootPath, ".verter", "statistics.json")
      : persistToFile
      ? join(process.cwd(), ".verter", "statistics.json")
      : undefined;

  return {
    enabled: config.get<boolean>("enabled") ?? false,
    persistToFile,
    filePath: configuredPath || defaultPath,
    maxSessionEntries: config.get<number>("maxSessionEntries") ?? undefined,
    maxPersistedEntries: config.get<number>("maxPersistedEntries") ?? undefined,
  };
}

function addDidChangeTextDocumentListener(getClient: GetClient) {
  workspace.onDidChangeTextDocument((e) => {
    if (
      e.document.languageId !== "typescript" &&
      e.document.languageId !== "javascript" &&
      e.document.languageId !== "vue"
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
      "verter.showCompiledCodeToSide",
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
function addWriteVirtualFilesCommand(
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
      "verter.writeVirtualFiles",
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

function addShowStatisticsCommand(
  getClient: GetClient,
  context: ExtensionContext
) {
  const channel = window.createOutputChannel("Verter Statistics");

  context.subscriptions.push(
    channel,
    commands.registerCommand("verter.showStatistics", async () => {
      try {
        const snapshot = await getClient().sendRequest(
          RequestType.GetStatistics,
          { includeEvents: false, scope: "all" }
        );

        if (!snapshot) {
          window.showWarningMessage(
            "Verter statistics are not available from the language server."
          );
          return;
        }

        channel.clear();
        renderStatisticsSnapshot(channel, snapshot);
        channel.show(true);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        window.showErrorMessage(
          `Failed to fetch Verter statistics: ${message}`
        );
      }
    })
  );
}

function renderStatisticsSnapshot(
  channel: OutputChannel,
  snapshot: StatisticsSnapshot
) {
  channel.appendLine(`Statistics ${snapshot.enabled ? "enabled" : "disabled"}`);
  channel.appendLine("");

  channel.appendLine("Session");
  channel.appendLine(
    formatSummarySection("  By type", snapshot.session.byType)
  );
  channel.appendLine(
    formatSummarySection("  By file", snapshot.session.byFile)
  );

  if (snapshot.global) {
    channel.appendLine("");
    const persistedLabel = snapshot.global.path
      ? `Global (persisted at ${snapshot.global.path})`
      : "Global";
    channel.appendLine(persistedLabel);
    channel.appendLine(
      formatSummarySection("  By type", snapshot.global.byType)
    );
    channel.appendLine(
      formatSummarySection("  By file", snapshot.global.byFile)
    );
  }
}

function formatSummarySection(
  title: string,
  summary: Record<string, StatisticsSummary>
) {
  const entries = Object.entries(summary ?? {});
  if (!entries.length) {
    return `${title}: none`;
  }

  const lines = entries.map(([key, value]) => formatSummaryLine(key, value));
  return [title, ...lines].join("\n");
}

function formatSummaryLine(key: string, summary: StatisticsSummary) {
  const average = summary.count ? summary.averageMs.toFixed(2) : "0";
  return `- ${key}: count=${
    summary.count
  }, avg=${average}ms, total=${summary.totalMs.toFixed(
    2
  )}ms, min=${summary.minMs.toFixed(2)}ms, max=${summary.maxMs.toFixed(2)}ms`;
}

function addNodeModulesChangedListener(getClient: GetClient) {
  const watchers = new Map<string, FileSystemWatcher>();
  function watchFolder(folder: WorkspaceFolder) {
    const fp = normalize(join(folder.uri.fsPath, "node_modules/**/*"));
    const watcher = workspace.createFileSystemWatcher(fp);
    watcher.onDidChange((e) => {
      console.log("changed", e.fsPath);
      getClient().sendNotification(NotificationType.OnFileChanged, {
        type: "update",
        uri: e.fsPath,
      });
    });
    watcher.onDidCreate((e) => {
      console.log("created", e.fsPath);
      getClient().sendNotification(NotificationType.OnFileChanged, {
        type: "create",
        uri: e.fsPath,
      });
    });
    watcher.onDidDelete((e) => {
      console.log("deleted", e.fsPath);
      getClient().sendNotification(NotificationType.OnFileChanged, {
        type: "delete",
        uri: e.fsPath,
      });
    });
    watchers.set(folder.uri.fsPath, watcher);
  }

  if (workspace.workspaceFolders) {
    workspace.workspaceFolders.forEach(watchFolder);
  }
  workspace.onDidChangeWorkspaceFolders((e) => {
    e.removed.forEach((folder) => {
      watchers.get(folder.uri.fsPath)?.dispose();
      watchers.delete(folder.uri.fsPath);
    });
    e.added.forEach(watchFolder);
  });
}
