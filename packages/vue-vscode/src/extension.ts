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
  LogOutputChannel,
  languages,
  type TextDocument,
  Diagnostic as VDiagnostic,
  Range as VRange,
  Position as VPosition,
  DiagnosticSeverity,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

import { join, normalize } from "path";
import { existsSync, appendFileSync, writeFileSync } from "fs";

import type { PatchClient } from "@verter/language-shared";
import { patchClient, NotificationType, RequestType } from "@verter/language-shared";
import type { StatisticsSnapshot, StatisticsSummary } from "@verter/language-shared";
import CompiledCodeContentProvider from "./CompiledCodeContentProvider";
import { VirtualFileContentProvider } from "./VirtualFileManager";
import { UnifiedVirtualFilesProvider } from "./UnifiedVirtualFilesProvider";
import type { UnifiedVirtualFileItem } from "./UnifiedVirtualFilesProvider";
import { ComponentTreeProvider } from "./ComponentTreeProvider";
import { AnalysisTreeProvider } from "./AnalysisTreeProvider";
import { VueApiDecorationProvider } from "./VueApiDecorationProvider";
import { BindingColorDecorationProvider } from "./BindingColorDecorationProvider";
import { PropConstnessDecorationProvider } from "./PropConstnessDecorationProvider";
import { SourceMapWebviewPanel } from "./SourceMapWebviewPanel";
import type { ComponentNode, ParentFileNode } from "./ComponentTreeProvider";
import { CssService } from "./css/cssService";
import { restartLanguageServer } from "./restart";
import { checkClaudeCodeAndNotify, setupMcpForClaudeCode } from "./claudeCodeDetection";

type GetClient = () => PatchClient<LanguageClient>;

let getClient: GetClient | undefined;

let activated = false;
let stopHeartbeat: (() => void) | undefined;

export async function activate(context: ExtensionContext) {
  if (activated) return;
  activated = true;

  const log = window.createOutputChannel("Verter", { log: true });
  context.subscriptions.push(log);

  // ── E2E test mode: dual-write logs to file + timing markers ──
  const testLogFile = process.env.VERTER_E2E_LOG_FILE;
  if (process.env.VERTER_E2E_TEST && testLogFile) {
    try { writeFileSync(testLogFile, ""); } catch {}
    const writeLog = (level: string, msg: string, ...args: unknown[]) => {
      const line = `[${level}] ${msg}${args.length ? " " + args.map(String).join(" ") : ""}\n`;
      try { appendFileSync(testLogFile, line); } catch {}
    };
    const origInfo = log.info.bind(log);
    const origWarn = log.warn.bind(log);
    const origError = log.error.bind(log);
    const origDebug = log.debug.bind(log);
    const origTrace = log.trace.bind(log);
    log.info = ((msg: string, ...args: unknown[]) => { writeLog("INFO", msg, ...args); origInfo(msg, ...args); }) as typeof log.info;
    log.warn = ((msg: string, ...args: unknown[]) => { writeLog("WARN", msg, ...args); origWarn(msg, ...args); }) as typeof log.warn;
    log.error = ((msg: string, ...args: unknown[]) => { writeLog("ERROR", msg, ...args); origError(msg, ...args); }) as typeof log.error;
    log.debug = ((msg: string, ...args: unknown[]) => { writeLog("DEBUG", msg, ...args); origDebug(msg, ...args); }) as typeof log.debug;
    log.trace = ((msg: string, ...args: unknown[]) => { writeLog("TRACE", msg, ...args); origTrace(msg, ...args); }) as typeof log.trace;
    writeLog("TIMING", `activation_start ${Date.now()}`);
  }

  log.info("Verter extension activating");

  const server = activateVueLanguageServer(context, log);
  getClient = server.getClient;
  stopHeartbeat = server.stopHeartbeatTimer;

  if (workspace.textDocuments.some((doc) => doc.languageId === "vue")) {
    commands.executeCommand(
      "_typescript.configurePlugin",
      require.resolve("@verter/typescript-plugin"),
      {
        enable: true,
      },
    );
  }
}

export function deactivate(): Thenable<void> | undefined {
  stopHeartbeat?.();
  stopHeartbeat = undefined;
  activated = false;
  const stop = getClient?.().stop();
  getClient = undefined;
  return stop;
}

/**
 * Find the verter-lsp binary.
 *
 * Search order:
 * 0. `VERTER_E2E_LSP_PATH` env var (E2E test mode — isolated copy)
 * 1. `verter.lspBinaryPath` setting (user-configured)
 * 2. `<monorepoRoot>/target/{debug,release}/verter-lsp[.exe]` (dev mode — newest wins)
 * 3. `<extensionPath>/bin/verter-lsp[.exe]` (bundled in VSIX)
 * 4. `verter-lsp` on PATH
 *
 * In development (running from the monorepo), the cargo build is preferred over the
 * bundled binary to ensure newly compiled changes take effect immediately.
 */
function findLspBinary(extensionPath: string, log: LogOutputChannel): string {
  const ext = process.platform === "win32" ? ".exe" : "";

  // 0. E2E test mode — use isolated binary copy to prevent file locking
  const e2eLspPath = process.env.VERTER_E2E_LSP_PATH;
  if (e2eLspPath && existsSync(e2eLspPath)) {
    log.info(`LSP binary: ${e2eLspPath} (E2E test copy)`);
    return e2eLspPath;
  }

  // 1. User-configured path
  const configuredPath = workspace.getConfiguration("verter").get<string>("lspBinaryPath");
  if (configuredPath && existsSync(configuredPath)) {
    log.info(`LSP binary: ${configuredPath} (user-configured)`);
    return configuredPath;
  }

  // 2. Development mode — cargo build output relative to extension path
  //    extensionPath is packages/vue-vscode, so monorepo root is ../../
  //    Prefer debug/release over bundled so `cargo build` changes are picked up.
  const monorepoRoot = join(extensionPath, "..", "..");
  for (const profile of ["debug", "release"]) {
    const cargoPath = join(monorepoRoot, "target", profile, `verter-lsp${ext}`);
    if (existsSync(cargoPath)) {
      log.info(`LSP binary: ${cargoPath} (dev ${profile})`);
      return cargoPath;
    }
  }

  // 3. Bundled binary (VSIX packaging)
  const bundledPath = join(extensionPath, "bin", `verter-lsp${ext}`);
  if (existsSync(bundledPath)) {
    log.info(`LSP binary: ${bundledPath} (bundled)`);
    return bundledPath;
  }

  // 4. Fall back to PATH
  log.info(`LSP binary: verter-lsp${ext} (PATH fallback)`);
  return `verter-lsp${ext}`;
}

export function activateVueLanguageServer(context: ExtensionContext, log: LogOutputChannel) {
  const { workspaceFolders } = workspace;
  const rootPath = Array.isArray(workspaceFolders) ? workspaceFolders[0].uri.fsPath : undefined;

  const binaryPath = findLspBinary(context.extensionPath, log);

  // CSS intellisense service — created after client, referenced by middleware closures
  let cssService: CssService | undefined;
  const cssDiagnostics = languages.createDiagnosticCollection("verter-css");
  context.subscriptions.push(cssDiagnostics);

  // Options to control the language client
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "vue" },
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "typescript" },
      // Virtual files from the Verter Analysis panel — route through the LSP
      // so it can provide position-mapped features (hover, definition, etc.)
      { scheme: VirtualFileContentProvider.scheme },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.{vue}"),
    },
    initializationOptions: {
      configuration: {
        vue: workspace.getConfiguration("vue"),
        oxcfmt: workspace.getConfiguration("oxcfmt"),
        emmet: workspace.getConfiguration("emmet"),
        typescript: workspace.getConfiguration("typescript"),
        javascript: workspace.getConfiguration("javascript"),
        css: workspace.getConfiguration("css"),
        less: workspace.getConfiguration("less"),
        scss: workspace.getConfiguration("scss"),
        html: workspace.getConfiguration("html"),
      },
      statistics: getStatisticsInitialization(rootPath),
      lint: {
        enabled: workspace.getConfiguration("verter").get<boolean>("lint.enabled", false),
        preset: workspace.getConfiguration("verter").get<string>("lint.preset", "recommended"),
      },
      viteConfig: {
        enabled: workspace.getConfiguration("verter").get<boolean>("viteConfig.enabled", true),
      },
    },
    outputChannel: log,
    traceOutputChannel: log,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    middleware: {
      provideCompletionItem: async (document, position, context, token, next) => {
        if (document.languageId !== "vue" || !cssService) {
          return next(document, position, context, token);
        }

        const source = document.getText();
        if (!cssService.isInStyleBlock(source, position.line, position.character)) {
          return next(document, position, context, token);
        }

        const uri = document.uri.toString();
        const [cssResult, lspResult] = await Promise.all([
          cssService.doComplete(uri, source, document.version, position.line, position.character),
          next(document, position, context, token),
        ]);

        if (!cssResult || !cssResult.items.length) return lspResult;

        // Convert CSS service result (LSP protocol types) → VS Code types
        const p2c = client.protocol2CodeConverter;
        const converted = await p2c.asCompletionResult(cssResult);

        if (!lspResult) return converted;

        // Merge: CSS items first, then LSP items (Vue-specific: template classes, v-bind)
        const cssItems = Array.isArray(converted) ? converted : converted?.items ?? [];
        const lspItems = Array.isArray(lspResult) ? lspResult : lspResult?.items ?? [];

        // Deduplicate by label
        const seen = new Set(cssItems.map((i) => i.label.toString()));
        const merged = [...cssItems, ...lspItems.filter((i) => !seen.has(i.label.toString()))];

        return { items: merged, isIncomplete: true };
      },

      provideHover: async (document, position, token, next) => {
        if (document.languageId !== "vue" || !cssService) {
          return next(document, position, token);
        }

        const source = document.getText();
        if (!cssService.isInStyleBlock(source, position.line, position.character)) {
          return next(document, position, token);
        }

        const uri = document.uri.toString();
        const [cssResult, lspResult] = await Promise.all([
          cssService.doHover(uri, source, document.version, position.line, position.character),
          next(document, position, token),
        ]);

        if (!cssResult) return lspResult;

        // Convert CSS hover → VS Code hover
        const p2c = client.protocol2CodeConverter;
        const cssHover = p2c.asHover(cssResult);

        if (!lspResult) return cssHover;

        // Merge: CSS docs first, then Vue-specific info
        if (cssHover && lspResult) {
          return {
            contents: [...cssHover.contents, ...lspResult.contents],
            range: cssHover.range ?? lspResult.range,
          };
        }
        return cssHover ?? lspResult;
      },

      provideDocumentColors: async (document, token, next) => {
        if (document.languageId !== "vue" || !cssService) {
          return next(document, token);
        }

        const source = document.getText();
        const uri = document.uri.toString();
        const [cssResult, lspResult] = await Promise.all([
          cssService.findDocumentColors(uri, source, document.version),
          next(document, token),
        ]);

        if (!cssResult.length) return lspResult;

        const p2c = client.protocol2CodeConverter;
        const cssColors = await p2c.asColorInformations(cssResult);

        if (!lspResult?.length) return cssColors;

        // Merge and deduplicate by range
        return [...(cssColors ?? []), ...(lspResult ?? [])];
      },

      provideColorPresentations: async (color, context, token, next) => {
        if (context.document.languageId !== "vue" || !cssService) {
          return next(color, context, token);
        }

        const source = context.document.getText();
        if (!cssService.isInStyleBlock(source, context.range.start.line, context.range.start.character)) {
          return next(color, context, token);
        }

        const uri = context.document.uri.toString();
        const cssResult = await cssService.getColorPresentations(
          uri,
          source,
          context.document.version,
          { red: color.red, green: color.green, blue: color.blue, alpha: color.alpha },
          context.range.start.line,
          context.range.start.character,
        );

        if (!cssResult.length) return next(color, context, token);

        const p2c = client.protocol2CodeConverter;
        return p2c.asColorPresentations(cssResult);
      },

      provideDocumentHighlights: async (document, position, token, next) => {
        if (document.languageId !== "vue" || !cssService) {
          return next(document, position, token);
        }

        const source = document.getText();
        if (!cssService.isInStyleBlock(source, position.line, position.character)) {
          return next(document, position, token);
        }

        const uri = document.uri.toString();
        const [cssResult, lspResult] = await Promise.all([
          cssService.findDocumentHighlights(uri, source, document.version, position.line, position.character),
          next(document, position, token),
        ]);

        if (!cssResult.length) return lspResult;

        const p2c = client.protocol2CodeConverter;
        const cssHighlights = await p2c.asDocumentHighlights(cssResult);

        if (!lspResult?.length) return cssHighlights;
        return [...(cssHighlights ?? []), ...(lspResult ?? [])];
      },
    },
  };

  let client = createLanguageServer(
    buildServerOptions(binaryPath, rootPath, context.extensionPath, log),
    clientOptions,
  );
  const getClient = () => client as unknown as PatchClient<LanguageClient>;

  // Track type provider child PID for orphan cleanup on restart failure.
  let typeProviderPid: number | undefined;
  function registerTypeProviderPidListener(lc: LanguageClient) {
    // New unified notification
    lc.onNotification(
      NotificationType.TypeProviderStarted,
      (params: { pid: number; kind: "tsgo" | "tsserver" }) => {
        typeProviderPid = params.pid;
        log.info(`Type provider (${params.kind}) started with PID ${params.pid}`);
      },
    );
    // Legacy notification for backward compat
    lc.onNotification(
      NotificationType.TsgoStarted,
      (params: { pid: number }) => {
        typeProviderPid = params.pid;
        log.info(`TSGO started with PID ${params.pid}`);
      },
    );
  }
  function killTrackedTypeProvider() {
    if (typeProviderPid != null) {
      log.info(`Killing orphaned type provider process (PID ${typeProviderPid})`);
      try {
        process.kill(typeProviderPid);
      } catch {
        // Already dead — ignore.
      }
      typeProviderPid = undefined;
    }
  }
  registerTypeProviderPidListener(client);

  // ── Heartbeat watchdog ──────────────────────────────────────────
  // The Rust server sends $/verter/heartbeat every 5 seconds. If we don't
  // receive one for 30 seconds, the server is likely frozen (e.g., tokio
  // runtime starvation from stdout pipe backpressure). Auto-restart it.
  //
  // Uses a 60s initial grace period to allow background initialization
  // (vite config eval, workspace scan) to complete before enforcing the
  // 30s timeout. The first heartbeat received switches to the normal 30s window.
  let heartbeatTimer: ReturnType<typeof setTimeout> | undefined;
  const HEARTBEAT_TIMEOUT_MS = 30_000;
  const HEARTBEAT_INITIAL_TIMEOUT_MS = 60_000;
  let heartbeatInitialized = false;

  function resetHeartbeatTimer() {
    if (heartbeatTimer) clearTimeout(heartbeatTimer);
    const timeout = heartbeatInitialized ? HEARTBEAT_TIMEOUT_MS : HEARTBEAT_INITIAL_TIMEOUT_MS;
    heartbeatInitialized = true; // After first reset from heartbeat, use normal timeout
    heartbeatTimer = setTimeout(async () => {
      log.error(`No heartbeat from Verter LSP for ${timeout / 1000}s — server appears frozen, restarting...`);
      await restartLS(false);
    }, timeout);
  }

  function registerHeartbeatMonitor(lc: LanguageClient) {
    // Start with initial grace period (60s) — background init may take time.
    // The first heartbeat received switches to the normal 30s timeout.
    heartbeatInitialized = false;
    if (heartbeatTimer) clearTimeout(heartbeatTimer);
    heartbeatTimer = setTimeout(async () => {
      log.error(`No heartbeat from Verter LSP for ${HEARTBEAT_INITIAL_TIMEOUT_MS / 1000}s — server appears frozen, restarting...`);
      await restartLS(false);
    }, HEARTBEAT_INITIAL_TIMEOUT_MS);
    lc.onNotification(NotificationType.Heartbeat, () => {
      resetHeartbeatTimer();
      // Log heartbeat in E2E test mode so tests can verify heartbeat receipt
      if (process.env.VERTER_E2E_TEST) {
        log.trace("$/verter/heartbeat received");
      }
    });
    lc.onNotification(NotificationType.Ready, (params: { gen: number }) => {
      log.info(`Verter ready (init generation ${params.gen})`);
      // E2E timing marker
      const testLogFile = process.env.VERTER_E2E_LOG_FILE;
      if (process.env.VERTER_E2E_TEST && testLogFile) {
        try { appendFileSync(testLogFile, `[TIMING] ready ${Date.now()}\n`); } catch {}
      }
    });
  }

  function stopHeartbeatTimer() {
    if (heartbeatTimer) {
      clearTimeout(heartbeatTimer);
      heartbeatTimer = undefined;
    }
  }

  registerHeartbeatMonitor(client);

  // Initialize CSS service now that getClient is available
  cssService = new CssService(getClient, rootPath);

  // CSS validation diagnostics — update on document change (debounced per URI)
  const cssDiagTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const updateCssDiagnostics = async (document: TextDocument) => {
    if (document.languageId !== "vue" || !cssService) return;
    try {
      const uri = document.uri.toString();
      const results = await cssService.doValidation(uri, document.getText(), document.version);
      const allDiags: VDiagnostic[] = [];
      for (const { diagnostics } of results) {
        for (const d of diagnostics) {
          allDiags.push(
            new VDiagnostic(
              new VRange(
                new VPosition(d.range.start.line, d.range.start.character),
                new VPosition(d.range.end.line, d.range.end.character),
              ),
              d.message,
              d.severity === 1
                ? DiagnosticSeverity.Error
                : d.severity === 2
                  ? DiagnosticSeverity.Warning
                  : DiagnosticSeverity.Information,
            ),
          );
        }
      }
      cssDiagnostics.set(document.uri, allDiags);
    } catch {
      // Silently fail — CSS diagnostics are best-effort
    }
  };
  const debouncedCssDiag = (document: TextDocument) => {
    const key = document.uri.toString();
    const existing = cssDiagTimers.get(key);
    if (existing) clearTimeout(existing);
    cssDiagTimers.set(
      key,
      setTimeout(() => {
        cssDiagTimers.delete(key);
        updateCssDiagnostics(document);
      }, 300),
    );
  };

  context.subscriptions.push(
    workspace.onDidChangeTextDocument((e) => {
      if (e.document.languageId === "vue") {
        debouncedCssDiag(e.document);
      }
    }),
    workspace.onDidOpenTextDocument(updateCssDiagnostics),
    workspace.onDidCloseTextDocument((doc) => cssDiagnostics.delete(doc.uri)),
  );

  context.subscriptions.push(
    commands.registerCommand("verter.restartLanguageServer", async () => {
      await restartLS(true);
    }),
  );

  let restarting = false;
  async function restartLS(showMsg: boolean) {
    if (restarting) {
      return;
    }
    restarting = true;
    try {
      const success = await restartLanguageServer({
        stop: () => client.stop(),
        createAndStart: async () => {
          client = createLanguageServer(
            buildServerOptions(binaryPath, rootPath, context.extensionPath, log),
            clientOptions,
          );
          registerTypeProviderPidListener(client);
          registerHeartbeatMonitor(client);
          await client.start();
        },
        killTrackedTypeProvider,
        resetServices: () => {
          cssService?.dispose();
          cssService = new CssService(getClient, rootPath);
          cssDiagnostics.clear();
        },
        log,
      });
      if (success && showMsg) {
        window.showInformationMessage("Verter Language server restarted");
      }
    } finally {
      restarting = false;
    }
  }

  // Auto-restart on settings changes that require server restart (debounced)
  let configRestartTimer: ReturnType<typeof setTimeout> | undefined;
  context.subscriptions.push(
    workspace.onDidChangeConfiguration((e) => {
      const needsRestart =
        e.affectsConfiguration("verter.server.logLevel") ||
        e.affectsConfiguration("verter.typeProvider") ||
        e.affectsConfiguration("verter.typescript.tsdk") ||
        e.affectsConfiguration("verter.mcp.enabled") ||
        e.affectsConfiguration("verter.mcp.port");
      if (needsRestart) {
        // Debounce: VS Code may fire multiple config change events in rapid succession
        if (configRestartTimer) clearTimeout(configRestartTimer);
        configRestartTimer = setTimeout(async () => {
          configRestartTimer = undefined;
          log.info("Settings changed, restarting language server...");
          await restartLS(false);
        }, 200);
      }
    }),
  );

  addDidChangeTextDocumentListener(getClient);
  addCompilePreviewCommand(getClient, context);

  addWriteVirtualFilesCommand(getClient, context);

  addShowStatisticsCommand(getClient, context, log);

  addNodeModulesChangedListener(getClient);

  addVerterAnalysis(getClient, context);

  // Claude Code detection notification
  checkClaudeCodeAndNotify(context, log);

  // Setup MCP for Claude Code command
  context.subscriptions.push(
    commands.registerCommand("verter.setupMcpForClaudeCode", () =>
      setupMcpForClaudeCode(context, log)
    )
  );

  return {
    getClient,
    stopHeartbeatTimer,
  };
}

function buildServerOptions(
  binaryPath: string,
  rootPath: string | undefined,
  extensionPath: string,
  log?: LogOutputChannel,
): ServerOptions {
  const logLevel = workspace.getConfiguration("verter.server").get<string>("logLevel", "info");
  const verterConfig = workspace.getConfiguration("verter");
  // E2E tests can override the type provider via environment variable
  const typeProvider = process.env.VERTER_E2E_TYPE_PROVIDER || verterConfig.get<string>("typeProvider", "auto");
  const tsdk = verterConfig.get<string>("typescript.tsdk", "");

  const mcpEnabled = verterConfig.get<boolean>("mcp.enabled", true);
  const mcpPort = verterConfig.get<number>("mcp.port", 6772);
  const mcpLintPreset = verterConfig.get<string>("mcp.lintPreset", "recommended");

  const args: string[] = [];
  args.push(`--type-provider=${typeProvider}`);
  if (tsdk) args.push(`--tsdk=${tsdk}`);
  args.push(`--plugin-path=${join(extensionPath, "node_modules")}`);
  if (mcpEnabled) {
    args.push(`--mcp-port=${mcpPort}`);
    args.push(`--mcp-lint-preset=${mcpLintPreset}`);
  }
  if (rootPath) args.push(rootPath);

  log?.info(`[buildServerOptions] typeProvider=${typeProvider}, tsdk=${tsdk}, args=${JSON.stringify(args)}`);

  return {
    run: {
      command: binaryPath,
      args,
      transport: TransportKind.stdio,
      options: {
        env: { ...process.env, VERTER_LOG: logLevel },
      },
    },
    debug: {
      command: binaryPath,
      args,
      transport: TransportKind.stdio,
      options: {
        env: { ...process.env, VERTER_LOG: "debug" },
      },
    },
  };
}

function createLanguageServer(serverOptions: ServerOptions, clientOptions: LanguageClientOptions) {
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
    // Only forward TS/JS changes — .vue changes are handled by the LSP's own did_change.
    // Sending .vue here would cause redundant notifications and TSGO flooding.
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

function addCompilePreviewCommand(getClient: GetClient, context: ExtensionContext) {
  const compiledCodeContentProvider = new CompiledCodeContentProvider(getClient);

  context.subscriptions.push(
    // Register the content provider for "vue-compiled://" files
    workspace.registerTextDocumentContentProvider(
      CompiledCodeContentProvider.scheme,
      compiledCodeContentProvider,
    ),
    compiledCodeContentProvider,
  );

  context.subscriptions.push(
    commands.registerTextEditorCommand("verter.showCompiledCodeToSide", async (editor) => {
      if (editor?.document?.languageId !== "vue") {
        window.showInformationMessage("Not a Vue file");
        return;
      }

      window.withProgress(
        { location: ProgressLocation.Window, title: "Compiling..." },
        async () => {
          // Open a new preview window for the compiled code
          return await window.showTextDocument(CompiledCodeContentProvider.previewWindowUri, {
            preview: true,
            viewColumn: ViewColumn.Beside,
          });
        },
      );
    }),
  );
}
function addWriteVirtualFilesCommand(getClient: GetClient, context: ExtensionContext) {
  const compiledCodeContentProvider = new CompiledCodeContentProvider(getClient);

  context.subscriptions.push(
    // Register the content provider for "vue-compiled://" files
    workspace.registerTextDocumentContentProvider(
      CompiledCodeContentProvider.scheme,
      compiledCodeContentProvider,
    ),
    compiledCodeContentProvider,
  );

  context.subscriptions.push(
    commands.registerTextEditorCommand("verter.writeVirtualFiles", async (editor) => {
      if (editor?.document?.languageId !== "vue") {
        window.showInformationMessage("Not a Vue file");
        return;
      }

      window.withProgress(
        { location: ProgressLocation.Window, title: "Compiling..." },
        async () => {
          // Open a new preview window for the compiled code
          return await window.showTextDocument(CompiledCodeContentProvider.previewWindowUri, {
            preview: true,
            viewColumn: ViewColumn.Beside,
          });
        },
      );
    }),
  );
}

function addShowStatisticsCommand(getClient: GetClient, context: ExtensionContext, log: LogOutputChannel) {
  const channel = window.createOutputChannel("Verter Statistics");

  context.subscriptions.push(
    channel,
    commands.registerCommand("verter.showStatistics", async () => {
      try {
        const snapshot = await getClient().sendRequest(RequestType.GetStatistics, {
          includeEvents: false,
          scope: "all",
        });

        if (!snapshot) {
          window.showWarningMessage(
            "Verter statistics are not available from the language server.",
          );
          return;
        }

        channel.clear();
        renderStatisticsSnapshot(channel, snapshot);
        channel.show(true);
      } catch (err) {
        log.error("Failed to fetch statistics", err as Error);
        const message = err instanceof Error ? err.message : String(err);
        window.showErrorMessage(`Failed to fetch Verter statistics: ${message}`);
      }
    }),
  );
}

function renderStatisticsSnapshot(channel: OutputChannel, snapshot: StatisticsSnapshot) {
  channel.appendLine(`Statistics ${snapshot.enabled ? "enabled" : "disabled"}`);
  channel.appendLine("");

  channel.appendLine("Session");
  channel.appendLine(formatSummarySection("  By type", snapshot.session.byType));
  channel.appendLine(formatSummarySection("  By file", snapshot.session.byFile));

  if (snapshot.global) {
    channel.appendLine("");
    const persistedLabel = snapshot.global.path
      ? `Global (persisted at ${snapshot.global.path})`
      : "Global";
    channel.appendLine(persistedLabel);
    channel.appendLine(formatSummarySection("  By type", snapshot.global.byType));
    channel.appendLine(formatSummarySection("  By file", snapshot.global.byFile));
  }
}

function formatSummarySection(title: string, summary: Record<string, StatisticsSummary>) {
  const entries = Object.entries(summary ?? {});
  if (!entries.length) {
    return `${title}: none`;
  }

  const lines = entries.map(([key, value]) => formatSummaryLine(key, value));
  return [title, ...lines].join("\n");
}

function formatSummaryLine(key: string, summary: StatisticsSummary) {
  const average = summary.count ? summary.averageMs.toFixed(2) : "0";
  return `- ${key}: count=${summary.count}, avg=${average}ms, total=${summary.totalMs.toFixed(
    2,
  )}ms, min=${summary.minMs.toFixed(2)}ms, max=${summary.maxMs.toFixed(2)}ms`;
}

function addVerterAnalysis(getClient: GetClient, context: ExtensionContext) {
  // Read config and set context for `when` clauses
  const updateAnalysisEnabled = () => {
    const enabled = workspace.getConfiguration("verter.analysis").get("enabled", false);
    commands.executeCommand("setContext", "verter.analysisEnabled", enabled);
  };
  updateAnalysisEnabled();
  context.subscriptions.push(
    workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("verter.analysis.enabled")) {
        updateAnalysisEnabled();
      }
    }),
  );

  // Track last active Vue file URI so sidebar persists when virtual files or non-.vue files are focused
  let lastVueFileUri: string | undefined;
  const getLastVueUri = () => lastVueFileUri;
  const updateLastVueFile = () => {
    const editor = window.activeTextEditor;
    if (editor?.document?.languageId === "vue") {
      lastVueFileUri = editor.document.uri.toString();
    }
  };
  updateLastVueFile();
  context.subscriptions.push(window.onDidChangeActiveTextEditor(updateLastVueFile));

  // Track whether active editor is a Vue file
  const updateHasActiveVueFile = () => {
    const isVue = window.activeTextEditor?.document?.languageId === "vue";
    commands.executeCommand("setContext", "verter.hasActiveVueFile", isVue);
  };
  updateHasActiveVueFile();
  context.subscriptions.push(window.onDidChangeActiveTextEditor(updateHasActiveVueFile));

  // Create content provider for virtual files (no disk writes — uses verter-virtual:// scheme)
  const contentProvider = new VirtualFileContentProvider();
  context.subscriptions.push(
    workspace.registerTextDocumentContentProvider(
      VirtualFileContentProvider.scheme,
      contentProvider,
    ),
    contentProvider,
  );

  const virtualFilesProvider = new UnifiedVirtualFilesProvider(getClient, contentProvider, getLastVueUri);
  const componentTreeProvider = new ComponentTreeProvider(getClient, getLastVueUri);
  const analysisProvider = new AnalysisTreeProvider(getClient, getLastVueUri);
  const decorationProvider = new VueApiDecorationProvider(getClient);
  const bindingColorProvider = new BindingColorDecorationProvider(getClient);
  const propConstnessProvider = new PropConstnessDecorationProvider(getClient);
  const sourceMapPanel = new SourceMapWebviewPanel();

  // ── E2E test mode: expose decoration state command ──────────
  if (process.env.VERTER_E2E_TEST) {
    context.subscriptions.push(
      commands.registerCommand("verter._getDecorationState", () => ({
        bindingColors: bindingColorProvider.getState(),
        vueApiCalls: decorationProvider.getState(),
        propConstness: propConstnessProvider.getState(),
      })),
    );
  }

  // Register tree views
  context.subscriptions.push(
    window.createTreeView("verterVirtualFiles", {
      treeDataProvider: virtualFilesProvider,
    }),
    window.createTreeView("verterComponentTree", {
      treeDataProvider: componentTreeProvider,
    }),
    window.createTreeView("verterAnalysis", {
      treeDataProvider: analysisProvider,
    }),
  );

  // Register commands
  context.subscriptions.push(
    commands.registerCommand("verter.openVirtualFile", (item: UnifiedVirtualFileItem) => {
      virtualFilesProvider.openVirtualFile(item);
    }),
    commands.registerCommand("verter.refreshVirtualFiles", () => {
      virtualFilesProvider.refresh();
    }),
    commands.registerCommand("verter.refreshAnalysis", () => {
      analysisProvider.refresh();
      componentTreeProvider.refresh();
    }),
    commands.registerCommand("verter.showSourceMapVisualization", async () => {
      const sourceUri = getLastVueUri();
      if (!sourceUri) {
        window.showInformationMessage("No Vue file active");
        return;
      }

      // Get source code from the open document or fall back to reading from disk
      const vueDoc = workspace.textDocuments.find(
        (d) => d.uri.toString() === sourceUri,
      );
      const sourceCode = vueDoc?.getText() ?? "";

      // Use cached items from the tree provider (already fetched)
      const items = virtualFilesProvider.getCachedItems();
      if (items.length === 0) {
        window.showInformationMessage("No virtual files available");
        return;
      }

      sourceMapPanel.show(sourceCode, sourceUri, items);
    }),
    commands.registerCommand(
      "verter.showSourceMapForFile",
      async (item: UnifiedVirtualFileItem) => {
        const sourceUri = item.sourceUri || getLastVueUri();
        if (!sourceUri) {
          window.showInformationMessage("No Vue file active");
          return;
        }

        const vueDoc = workspace.textDocuments.find(
          (d) => d.uri.toString() === sourceUri,
        );
        const sourceCode = vueDoc?.getText() ?? "";

        const items = virtualFilesProvider.getCachedItems();
        if (items.length === 0) {
          window.showInformationMessage("No virtual files available");
          return;
        }

        // Find the tab index matching the clicked item
        const tabIndex = items
          .filter((vf) => vf.sourceMap)
          .findIndex((vf) => vf.kind === item.kind);

        sourceMapPanel.show(sourceCode, sourceUri, items, Math.max(0, tabIndex));
      },
    ),
    commands.registerCommand("verter.goToComponent", (node: ComponentNode) => {
      componentTreeProvider.goToComponent(node);
    }),
    commands.registerCommand("verter.goToParentFile", (node: ParentFileNode) => {
      componentTreeProvider.goToParentFile(node);
    }),
  );

  // Cleanup on deactivate
  context.subscriptions.push({
    dispose() {
      virtualFilesProvider.dispose();
      componentTreeProvider.dispose();
      analysisProvider.dispose();
      decorationProvider.dispose();
      bindingColorProvider.dispose();
      propConstnessProvider.dispose();
      sourceMapPanel.dispose();
    },
  });
}

function addNodeModulesChangedListener(getClient: GetClient) {
  const watchers = new Map<string, FileSystemWatcher>();
  function watchFolder(folder: WorkspaceFolder) {
    const fp = normalize(join(folder.uri.fsPath, "node_modules/**/*"));
    const watcher = workspace.createFileSystemWatcher(fp);
    watcher.onDidChange((e) => {
      getClient().sendNotification(NotificationType.OnFileChanged, {
        type: "update",
        uri: e.fsPath,
      });
    });
    watcher.onDidCreate((e) => {
      getClient().sendNotification(NotificationType.OnFileChanged, {
        type: "create",
        uri: e.fsPath,
      });
    });
    watcher.onDidDelete((e) => {
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
