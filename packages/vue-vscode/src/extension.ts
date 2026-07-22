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
  StatusBarAlignment,
  StatusBarItem,
  languages,
  lm,
  Uri,
  McpHttpServerDefinition,
  type Disposable,
  type TextDocument,
  Diagnostic as VDiagnostic,
  Range as VRange,
  Position as VPosition,
  DiagnosticSeverity,
  ConfigurationTarget,
  ThemeColor,
  extensions,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

import { basename, join, normalize, resolve, sep } from "path";
import { appendFileSync, existsSync, readdirSync, rmSync, statSync, writeFileSync } from "fs";
import { tmpdir } from "os";

import type { PatchClient, NotificationParams } from "@verter/language-shared";
import {
  CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY,
  EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY,
  EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY,
  E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY,
  patchClient,
  NotificationType,
  RequestType,
} from "@verter/language-shared";
import type { StatisticsSnapshot, StatisticsSummary } from "@verter/language-shared";
import { computeProviderRecommendationNotice, computeStatusBarState } from "./statusBar";
import CompiledCodeContentProvider from "./CompiledCodeContentProvider";
import { VirtualFileContentProvider } from "./VirtualFileManager";
import { UnifiedVirtualFilesProvider } from "./UnifiedVirtualFilesProvider";
import type { UnifiedVirtualFileItem } from "./UnifiedVirtualFilesProvider";
import { ComponentTreeProvider } from "./ComponentTreeProvider";
import { RouteTreeProvider } from "./RouteTreeProvider";
import { AnalysisTreeProvider } from "./AnalysisTreeProvider";
import { VueApiDecorationProvider } from "./VueApiDecorationProvider";
import { BindingColorDecorationProvider } from "./BindingColorDecorationProvider";
import { PropConstnessDecorationProvider } from "./PropConstnessDecorationProvider";
import { SourceMapWebviewPanel } from "./SourceMapWebviewPanel";
import type { ComponentNode, ParentFileNode } from "./ComponentTreeProvider";
import { CssService } from "./css/cssService";
import { findStyleBlockAt, scanStyleBlocks } from "./css/styleBlockScanner";
import { restartLanguageServer } from "./restart";
import {
  checkClaudeCodeAndNotify,
  setupMcpForClaudeCode,
  updateMcpPort,
} from "./claudeCodeDetection";
import { createActivationGate } from "./activationGate";
import { readE2eEnv } from "./e2eEnv";
import { installE2eLogMirror } from "./e2eLogMirror";
import {
  frameworkDocumentSelector,
  frameworkClientLanguageIds,
  isFrameworkCarrierLanguageId,
  shouldConfigureTypeScriptPluginForLanguageId,
} from "./frameworkWiring";
import { StartupProbe, readStartupProbeConfig, writeTimingMarker } from "./startupProbe";
import { shouldRestartLanguageServerForConfigurationChange } from "./languageServerConfig";
import { addShowRecentAuditRecordsCommand } from "./audit";
import {
  buildRelayEditorEnv,
  isShimAdvertisement,
  isVerterSharedControlDirName,
  nativePreviewTsdkCandidates,
  orphanedControlDirs,
  planSharedTsgo,
  prepareEditorTsdk,
  typeProviderRoutesTsgo,
} from "./sharedTsgoLaunch";
import {
  NativePreviewRelayController,
  type NativePreviewApi,
} from "./nativePreviewRelayController";
import {
  attestEditorTsserverBootstrap,
  editorTsserverOwnsCarrierSourceFeatures,
  VERTER_TYPESCRIPT_PLUGIN_ID,
  planEditorTsserverBootstrap,
  receiptIncludesConfiguredProject,
  selectEditorTsserverBootstrapCarrier,
  typeProviderRoutesEditorTsserver,
} from "./editorTsserverBootstrap";

type GetClient = () => PatchClient<LanguageClient>;
type ActivationRuntime = Awaited<ReturnType<typeof activateExtension>>;

let getClient: GetClient | undefined;
let stopHeartbeat: (() => void) | undefined;
let activationContext: ExtensionContext | undefined;
const activationGate = createActivationGate<ActivationRuntime>(async () => {
  if (!activationContext) {
    throw new Error("Verter activation context was not initialized");
  }

  const runtime = await activateExtension(activationContext);
  getClient = runtime.getClient;
  stopHeartbeat = runtime.stopHeartbeatTimer;
  return runtime;
});

export async function activate(context: ExtensionContext) {
  activationContext = context;
  await activationGate.run();
}

async function activateExtension(context: ExtensionContext) {
  const log = window.createOutputChannel("Verter", { log: true });
  context.subscriptions.push(log);

  // ── E2E test mode: dual-write logs to file + timing markers ──
  const testLogFile = process.env.VERTER_E2E_LOG_FILE;
  if (process.env.VERTER_E2E_TEST && testLogFile) {
    try {
      writeFileSync(testLogFile, "");
    } catch {}
    installE2eLogMirror(log, (text) => {
      try {
        appendFileSync(testLogFile, text);
      } catch {}
    });
  }
  writeTimingMarker("activation_start", Date.now());

  log.info("Verter extension activating");
  const e2eProviderOnlyCompletions =
    process.env.VERTER_E2E_TEST === "1" && process.env.VERTER_E2E_PROVIDER_ONLY_COMPLETIONS === "1";

  const startupProbeConfig = readStartupProbeConfig();
  const startupProbe = startupProbeConfig ? new StartupProbe(startupProbeConfig, log) : undefined;
  if (startupProbe) {
    context.subscriptions.push(startupProbe);
  }

  let server:
    | (Awaited<ReturnType<typeof activateVueLanguageServer>> & {
        restart(showMsg: boolean): Promise<void>;
      })
    | undefined;
  let serverPromise:
    | Promise<
        Awaited<ReturnType<typeof activateVueLanguageServer>> & {
          restart(showMsg: boolean): Promise<void>;
        }
      >
    | undefined;
  let clientListenersRegistered = false;
  let deferredFeaturesRegistered = false;
  let configRestartTimer: ReturnType<typeof setTimeout> | undefined;
  let tsPluginConfigured = false;
  let tsPluginPromise: Promise<void> | undefined;
  let tsPluginRefreshRequested = false;
  // The resolved per-workspace carrier-store dir the LSP publishes carriers into,
  // delivered by the LSP's `$/verter/carrierStoreReady` notification (emitted from
  // the server's init lifecycle, which resolves the dir authoritatively via
  // `default_carrier_store_dir_string(workspace_root)` and also hands it to its own
  // spawned tsserver through `VERTER_CARRIER_STORE_DIR`). The notification handler
  // (`onCarrierStoreReady` → `applyCarrierStoreDir`) records it here and forwards it
  // to VS Code's OWN TypeScript server via `configurePlugin`, so a plain `.ts`
  // (served by VS Code's TS service, not the LSP-spawned tsserver) reads the SAME
  // store the LSP writes — the headline "plain .ts importing a .vue gets real types"
  // DX. The extension never re-derives the dir itself (the recipe — blake3 over the
  // canonicalized + case-folded workspace root plus the LSP version — lives solely
  // in the LSP, so mirroring it here would risk silently targeting the WRONG dir).
  // `undefined` until the notification arrives; until then VS Code's own TS server
  // has no store and fails closed (the LSP-spawned tsserver still has the env dir).
  let carrierStoreDir: string | undefined;
  // The store directory remains stable across publications. Advance this token
  // only after the LSP reports that its current carrier generation has fully
  // synchronized, causing the editor tsserver plugin to reload that configured
  // project's external roots and snapshots exactly once.
  let carrierStoreRefreshToken = 0;
  // Membership/import resolution remains editor-owned on every route. Source
  // features become editor-owned only after an exact tsserver project attests;
  // managed/shared tsgo must not be merged with a second carrier provider.
  let editorOwnsCarrierSourceFeatures = false;

  const getStartedClient = () => {
    if (!server) {
      throw new Error("Verter language server is not started");
    }
    return server.getClient();
  };

  const compiledCodeContentProvider = new CompiledCodeContentProvider(getStartedClient);
  context.subscriptions.push(
    workspace.registerTextDocumentContentProvider(
      CompiledCodeContentProvider.scheme,
      compiledCodeContentProvider,
    ),
    compiledCodeContentProvider,
  );

  const getTypeScriptPluginConfig = (): {
    enable: true;
    editorOwnsCarrierMembership: true;
    editorOwnsCarrierSourceFeatures: boolean;
    e2eProviderOnlyCompletions: boolean;
    carrierStoreRefreshToken: number;
    exposeBindingsTesting?: boolean;
    carrierStoreDir?: string;
  } => {
    const pluginConfig: {
      enable: true;
      editorOwnsCarrierMembership: true;
      editorOwnsCarrierSourceFeatures: boolean;
      e2eProviderOnlyCompletions: boolean;
      carrierStoreRefreshToken: number;
      exposeBindingsTesting?: boolean;
      carrierStoreDir?: string;
    } = {
      enable: true,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: editorOwnsCarrierSourceFeatures,
      [E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY]: e2eProviderOnlyCompletions,
      [CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY]: carrierStoreRefreshToken,
    };
    const experimentalConfig = workspace.getConfiguration("verter.experimental");
    const inspect = experimentalConfig.inspect<boolean>("exposeBindingsTesting");
    const hasExplicitValue =
      inspect?.globalValue !== undefined ||
      inspect?.workspaceValue !== undefined ||
      inspect?.workspaceFolderValue !== undefined ||
      inspect?.globalLanguageValue !== undefined ||
      inspect?.workspaceLanguageValue !== undefined ||
      inspect?.workspaceFolderLanguageValue !== undefined;

    if (hasExplicitValue) {
      pluginConfig.exposeBindingsTesting = experimentalConfig.get<boolean>(
        "exposeBindingsTesting",
        false,
      );
    }

    // Forward the LSP-reported carrier-store dir to VS Code's TS server plugin
    // so it reads the same store the LSP publishes carriers into. Omitted until
    // the LSP reports it (the plugin then falls back to the env var).
    if (carrierStoreDir !== undefined) {
      pluginConfig.carrierStoreDir = carrierStoreDir;
    }

    return pluginConfig;
  };

  /**
   * Record the LSP-reported carrier-store dir and (re-)configure VS Code's TS
   * server plugin so it picks up the new `carrierStoreDir`. A no-op when the dir
   * is unchanged (avoids a redundant `configurePlugin` round-trip). When the dir
   * changes (first report, or a workspace switch), the configured flag is reset
   * so `ensureTypeScriptPluginConfigured(force)` re-issues `configurePlugin` with
   * the updated config.
   */
  const applyCarrierStoreDir = (dir: string | undefined) => {
    if (dir === carrierStoreDir) {
      return;
    }
    carrierStoreDir = dir;
    tsPluginConfigured = false;
    tsPluginPromise = undefined;
    void ensureTypeScriptPluginConfigured(undefined, true);
  };

  const ensureTypeScriptPluginConfigured = (document?: TextDocument, force = false) => {
    if (tsPluginPromise) {
      if (force) tsPluginRefreshRequested = true;
      return tsPluginPromise;
    }
    if (
      (!force && tsPluginConfigured) ||
      (!force && !shouldConfigureTypeScriptPluginForLanguageId(document?.languageId))
    ) {
      return tsPluginPromise;
    }

    writeTimingMarker("ts_plugin_configure_start", Date.now());
    tsPluginPromise = Promise.resolve(
      commands.executeCommand(
        "_typescript.configurePlugin",
        VERTER_TYPESCRIPT_PLUGIN_ID,
        getTypeScriptPluginConfig(),
      ),
    )
      .then(() => {
        tsPluginConfigured = true;
      })
      .catch((error: unknown) => {
        tsPluginConfigured = false;
        log.warn("Failed to configure the Verter TypeScript plugin", error);
      })
      .finally(() => {
        writeTimingMarker(
          "ts_plugin_configure_end",
          Date.now(),
          tsPluginConfigured ? "configured" : "retry",
        );
        tsPluginPromise = undefined;
        if (tsPluginRefreshRequested) {
          tsPluginRefreshRequested = false;
          tsPluginConfigured = false;
          tsPluginPromise = undefined;
          void ensureTypeScriptPluginConfigured(undefined, true);
        }
      });

    return tsPluginPromise;
  };

  const ensureDeferredFeaturesRegistered = () => {
    if (deferredFeaturesRegistered) {
      return;
    }
    deferredFeaturesRegistered = true;
    context.subscriptions.push(addNodeModulesChangedListener(getStartedClient));
    context.subscriptions.push(addViteConfigChangedListener(getStartedClient));
    addVerterAnalysis(getStartedClient, context);
    setTimeout(() => {
      checkClaudeCodeAndNotify(context, log);
    }, 0);
  };

  const ensureLanguageServerStarted = async () => {
    if (server) {
      return server;
    }
    if (serverPromise) {
      return serverPromise;
    }

    serverPromise = activateVueLanguageServer(context, log, startupProbe, {
      onReady: ensureDeferredFeaturesRegistered,
      onCarrierStoreReady: applyCarrierStoreDir,
      onEditorCarrierSourceFeatureOwnership: (ownsSourceFeatures) => {
        if (editorOwnsCarrierSourceFeatures === ownsSourceFeatures) return;
        editorOwnsCarrierSourceFeatures = ownsSourceFeatures;
        tsPluginConfigured = false;
        void ensureTypeScriptPluginConfigured(undefined, true);
      },
      onTypeProviderSyncComplete: () => {
        carrierStoreRefreshToken += 1;
        void ensureTypeScriptPluginConfigured(undefined, true);
      },
    })
      .then((runtime) => {
        server = runtime;
        if (!clientListenersRegistered) {
          context.subscriptions.push(addDidChangeTextDocumentListener(getStartedClient));
          clientListenersRegistered = true;
        }
        return runtime;
      })
      .catch((error) => {
        serverPromise = undefined;
        throw error;
      });

    return serverPromise;
  };

  const ensureStartedForFrameworkDocument = (document?: TextDocument) => {
    if (!isFrameworkCarrierLanguageId(document?.languageId)) {
      return;
    }
    void ensureLanguageServerStarted();
  };

  const ensureTypeScriptPluginConfiguredForDocument = (document?: TextDocument) => {
    void ensureTypeScriptPluginConfigured(document);
  };

  context.subscriptions.push(
    workspace.onDidOpenTextDocument((document) => {
      ensureTypeScriptPluginConfiguredForDocument(document);
      ensureStartedForFrameworkDocument(document);
    }),
    window.onDidChangeActiveTextEditor((editor) => {
      ensureTypeScriptPluginConfiguredForDocument(editor?.document);
      ensureStartedForFrameworkDocument(editor?.document);
    }),
    workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("verter.experimental.exposeBindingsTesting")) {
        tsPluginConfigured = false;
        tsPluginPromise = undefined;
        void ensureTypeScriptPluginConfigured(undefined, true);
      }

      const needsRestart = shouldRestartLanguageServerForConfigurationChange(e);
      if (!needsRestart || !server) {
        return;
      }

      if (configRestartTimer) {
        clearTimeout(configRestartTimer);
      }
      configRestartTimer = setTimeout(async () => {
        configRestartTimer = undefined;
        log.info("Settings changed, restarting language server...");
        await server?.restart(false);
      }, 200);
    }),
  );

  addCompilePreviewCommand(context, ensureLanguageServerStarted);
  addWriteVirtualFilesCommand(context, ensureLanguageServerStarted);
  addShowStatisticsCommand(context, log, ensureLanguageServerStarted, getStartedClient);
  addShowRecentAuditRecordsCommand(context, log, ensureLanguageServerStarted, getStartedClient);

  context.subscriptions.push(
    commands.registerCommand("verter.showOutputChannel", () => log.show()),
    commands.registerCommand("verter.restartLanguageServer", async () => {
      if (!server) {
        await ensureLanguageServerStarted();
        window.showInformationMessage("Verter Language server started");
        return;
      }
      await server.restart(true);
    }),
    commands.registerCommand("verter.setupMcpForClaudeCode", () =>
      setupMcpForClaudeCode(context, log),
    ),
  );

  if (
    workspace.textDocuments.some((doc) => isFrameworkCarrierLanguageId(doc.languageId)) ||
    isFrameworkCarrierLanguageId(window.activeTextEditor?.document.languageId)
  ) {
    void ensureLanguageServerStarted();
  }
  ensureTypeScriptPluginConfiguredForDocument(window.activeTextEditor?.document);

  return {
    getClient: getStartedClient,
    stopHeartbeatTimer: () => {
      server?.stopHeartbeatTimer();
    },
  };
}

export function deactivate(): Thenable<void> | undefined {
  stopHeartbeat?.();
  stopHeartbeat = undefined;
  activationContext = undefined;
  activationGate.reset();
  try {
    return getClient?.().stop();
  } catch {
    return undefined;
  } finally {
    getClient = undefined;
  }
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

export async function activateVueLanguageServer(
  context: ExtensionContext,
  log: LogOutputChannel,
  startupProbe?: StartupProbe,
  options?: {
    onReady?: () => void;
    /**
     * Called with the LSP-reported per-workspace carrier-store dir (the
     * `CarrierStoreReady` notification). The extension forwards it to VS Code's
     * own TS server via `configurePlugin`.
     */
    onCarrierStoreReady?: (carrierStoreDir: string) => void;
    /** Select exactly one editor-facing TypeScript owner for carrier features. */
    onEditorCarrierSourceFeatureOwnership?: (ownsSourceFeatures: boolean) => void;
    /** Refresh the editor plugin after a durable carrier-store publication pass. */
    onTypeProviderSyncComplete?: () => void;
  },
) {
  const { workspaceFolders } = workspace;
  const rootPath = Array.isArray(workspaceFolders) ? workspaceFolders[0].uri.fsPath : undefined;

  const binaryPath = findLspBinary(context.extensionPath, log);

  // Try the exact editor-owned Native Preview Program first. Native Preview launches
  // the staged relay as its own language server, and its public API attests that exact
  // session before the Verter LSP is armed.
  const effectiveTypeProvider =
    readE2eEnv("TYPE_PROVIDER") ||
    workspace.getConfiguration("verter").get<string>("typeProvider", "auto");
  const sharedTsgo = await establishSharedTsgo(
    context.extensionPath,
    rootPath,
    effectiveTypeProvider,
    log,
  );
  context.subscriptions.push({ dispose: () => sharedTsgo.dispose() });
  const editorTsserver =
    sharedTsgo.lspArgs.length === 0
      ? await establishEditorTsserverPlugin(effectiveTypeProvider, rootPath, log)
      : NO_EDITOR_TSSERVER;
  context.subscriptions.push({ dispose: () => editorTsserver.dispose() });
  options?.onEditorCarrierSourceFeatureOwnership?.(
    editorTsserverOwnsCarrierSourceFeatures(editorTsserver.lspArgs),
  );

  // CSS intellisense service — created after client, referenced by middleware closures
  let cssService: CssService | undefined;
  const getCssService = () => {
    if (!cssService) {
      writeTimingMarker("css_service_construct_start", Date.now());
      cssService = new CssService(getClient, rootPath);
      writeTimingMarker("css_service_construct_end", Date.now());
    }
    return cssService;
  };
  const cssDiagnostics = languages.createDiagnosticCollection("verter-css");
  context.subscriptions.push(cssDiagnostics);
  const hasStyleBlockAtPosition = (source: string, line: number, character: number) =>
    findStyleBlockAt(scanStyleBlocks(source), source, line, character) !== undefined;
  const hasStyleBlocks = (source: string) => scanStyleBlocks(source).length > 0;

  // The active framework carriers are EVERY registered adapter — derived from
  // the descriptor-generated client framework manifest, NOT an opt-in client
  // gate. Svelte is first-class (no opt-in). Each attaches to its manifest
  // client language id, and Verter contributes the language configuration and
  // TextMate grammar for both carriers (source.vue / source.svelte).
  const activeFrameworks = frameworkClientLanguageIds();

  // Options to control the language client
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      // The base TS/JS surface + every registered framework's client language
      // id, derived from the manifest (one `{ scheme: "file", language }` each).
      ...frameworkDocumentSelector(),
      // Virtual files from the Verter Analysis panel — route through the LSP
      // so it can provide position-mapped features (hover, definition, etc.)
      { scheme: VirtualFileContentProvider.scheme },
    ],
    // File watching is handled server-side via dynamic registration of
    // workspace/didChangeWatchedFiles (covers .vue, .ts/.js, config files).
    // No client-side synchronize.fileEvents needed.
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
      // Every registered framework carrier the client wires (manifest-derived).
      frameworks: activeFrameworks,
      lint: {
        enabled: workspace.getConfiguration("verter").get<boolean>("lint.enabled", false),
        preset: workspace.getConfiguration("verter").get<string>("lint.preset", "recommended"),
      },
      viteConfig: {
        enabled: workspace.getConfiguration("verter").get<boolean>("viteConfig.enabled", true),
        trustedFiles: workspace
          .getConfiguration("verter")
          .get<string[]>("viteConfig.trustedFiles", []),
      },
      inlayHints: {
        enabled: workspace.getConfiguration("verter").get<boolean>("inlayHints.enabled", true),
      },
      experimental: {
        conditionalRootNarrowing: workspace
          .getConfiguration("verter.experimental")
          .get<boolean>("conditionalRootNarrowing", false),
        strictSlots: workspace
          .getConfiguration("verter.experimental")
          .get<boolean>("strictSlots", false),
      },
    },
    outputChannel: log,
    traceOutputChannel: log,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    middleware: {
      provideCompletionItem: async (document, position, context, token, next) => {
        if (!isFrameworkCarrierLanguageId(document.languageId)) {
          return next(document, position, context, token);
        }

        const source = document.getText();
        if (!hasStyleBlockAtPosition(source, position.line, position.character)) {
          return next(document, position, context, token);
        }

        const cssService = getCssService();
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
        const cssItems = Array.isArray(converted) ? converted : (converted?.items ?? []);
        const lspItems = Array.isArray(lspResult) ? lspResult : (lspResult?.items ?? []);

        // Deduplicate by label
        const seen = new Set(cssItems.map((i) => i.label.toString()));
        const merged = [...cssItems, ...lspItems.filter((i) => !seen.has(i.label.toString()))];

        return { items: merged, isIncomplete: true };
      },

      provideHover: async (document, position, token, next) => {
        if (!isFrameworkCarrierLanguageId(document.languageId)) {
          return next(document, position, token);
        }

        const source = document.getText();
        if (!hasStyleBlockAtPosition(source, position.line, position.character)) {
          return next(document, position, token);
        }

        const cssService = getCssService();
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
        if (!isFrameworkCarrierLanguageId(document.languageId)) {
          return next(document, token);
        }

        const source = document.getText();
        if (!hasStyleBlocks(source)) {
          return next(document, token);
        }

        const uri = document.uri.toString();
        const cssService = getCssService();
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
        if (!isFrameworkCarrierLanguageId(context.document.languageId)) {
          return next(color, context, token);
        }

        const source = context.document.getText();
        if (
          !hasStyleBlockAtPosition(source, context.range.start.line, context.range.start.character)
        ) {
          return next(color, context, token);
        }

        const cssService = getCssService();
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
        if (!isFrameworkCarrierLanguageId(document.languageId)) {
          return next(document, position, token);
        }

        const source = document.getText();
        if (!hasStyleBlockAtPosition(source, position.line, position.character)) {
          return next(document, position, token);
        }

        const cssService = getCssService();
        const uri = document.uri.toString();
        const [cssResult, lspResult] = await Promise.all([
          cssService.findDocumentHighlights(
            uri,
            source,
            document.version,
            position.line,
            position.character,
          ),
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
    buildServerOptions(binaryPath, rootPath, context.extensionPath, log, [
      ...sharedTsgo.lspArgs,
      ...editorTsserver.lspArgs,
    ]),
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
        startupProbe?.markTypeProviderStarted(params.kind);
      },
    );
    // Legacy notification — only sent when TSGO is actually active
    lc.onNotification(NotificationType.TsgoStarted, (params: { pid: number }) => {
      typeProviderPid = params.pid;
    });
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

  // ── MCP server auto-registration ────────────────────────────────
  // When the MCP HTTP server binds a dynamic port, it sends $/verter/mcpReady.
  // We register it with VS Code's MCP provider API so Copilot Chat discovers it,
  // and update .mcp.json for Claude Code CLI.
  let mcpProviderDisposable: Disposable | undefined;
  function registerMcpListener(lc: LanguageClient) {
    lc.onNotification(NotificationType.McpReady, (params: { port: number }) => {
      log.info(`MCP HTTP server ready on port ${params.port}`);

      // Register with VS Code's MCP provider API (Copilot Chat auto-discovery)
      try {
        mcpProviderDisposable?.dispose();
        mcpProviderDisposable = lm.registerMcpServerDefinitionProvider("verter", {
          provideMcpServerDefinitions() {
            return [
              new McpHttpServerDefinition(
                "Verter Vue Analysis",
                Uri.parse(`http://localhost:${params.port}/mcp`),
              ),
            ];
          },
        });
        context.subscriptions.push(mcpProviderDisposable);
        log.info("Registered MCP server with VS Code MCP provider API");
      } catch (e) {
        log.warn(`Failed to register MCP server with VS Code: ${e}`);
      }

      // Update .mcp.json for Claude Code CLI
      const wsRoot = workspace.workspaceFolders?.[0]?.uri.fsPath;
      if (wsRoot) {
        updateMcpPort(wsRoot, params.port, log);
      }
    });
  }
  registerMcpListener(client);

  // ── Vite config trust prompt ────────────────────────────────────
  // When the LSP sends $/verter/viteConfigTrustRequired, show a warning
  // prompting the user to trust the file for execution.
  const promptedViteConfigs = new Set<string>();

  function registerViteConfigTrustHandler(lc: LanguageClient) {
    lc.onNotification(
      NotificationType.ViteConfigTrustRequired,
      async (params: { configPath: string; workspaceRoot: string; reason: string }) => {
        if (promptedViteConfigs.has(params.configPath)) return;
        promptedViteConfigs.add(params.configPath);

        const action = await window.showWarningMessage(
          `Verter cannot statically analyze ${params.configPath}. Trust this file for execution?`,
          "Trust File",
          "Open File",
          "Disable Vite Discovery",
        );

        if (action === "Trust File") {
          const config = workspace.getConfiguration("verter");
          const existing = config.get<string[]>("viteConfig.trustedFiles", []);
          if (!existing.includes(params.configPath)) {
            await config.update(
              "viteConfig.trustedFiles",
              [...existing, params.configPath],
              ConfigurationTarget.Workspace,
            );
            // Restart is triggered by the config change watcher
          }
        } else if (action === "Open File") {
          const doc = await workspace.openTextDocument(Uri.file(params.configPath));
          await window.showTextDocument(doc);
        } else if (action === "Disable Vite Discovery") {
          await workspace
            .getConfiguration("verter")
            .update("viteConfig.enabled", false, ConfigurationTarget.Workspace);
          // Restart is triggered by the config change watcher
        }
      },
    );
  }
  registerViteConfigTrustHandler(client);

  // ── Type provider status bar ────────────────────────────────────
  // Shows which type provider is active. Warning state when none is available.
  const typeProviderStatusBar: StatusBarItem = window.createStatusBarItem(
    StatusBarAlignment.Right,
    98,
  );
  typeProviderStatusBar.command = "verter.showOutputChannel";
  typeProviderStatusBar.text = "$(sync~spin) Verter";
  typeProviderStatusBar.tooltip = "Verter: waiting for type provider status...";
  typeProviderStatusBar.show();
  context.subscriptions.push(typeProviderStatusBar);

  // Provider recommendation (tsgo-preferred): the server sends structured
  // facts on $/verter/typeProviderStatus; this client renders them once per
  // workspace, dismissible, gated on verter.providerRecommendations.
  const PROVIDER_RECOMMENDATION_DISMISSED_KEY = "verter.providerRecommendation.dismissed";
  let providerRecommendationShownThisSession = false;
  function maybeShowProviderRecommendation(
    params: NotificationParams[typeof NotificationType.TypeProviderStatus],
  ) {
    const notice = computeProviderRecommendationNotice(params, {
      enabled: workspace.getConfiguration("verter").get<boolean>("providerRecommendations", true),
      dismissed:
        providerRecommendationShownThisSession ||
        context.workspaceState.get<boolean>(PROVIDER_RECOMMENDATION_DISMISSED_KEY, false),
    });
    if (!notice) return;
    providerRecommendationShownThisSession = true;
    // Logged for observability (and E2E attestation of the route behavior):
    // the notice fires exactly on tsserver-family serving, never on tsgo.
    log.info(`Provider recommendation: ${notice.message}`);
    void window
      .showInformationMessage(notice.message, "Open Settings", "Don't show again")
      .then((choice) => {
        if (choice === "Open Settings") {
          void commands.executeCommand("workbench.action.openSettings", "verter.typeProvider");
        } else if (choice === "Don't show again") {
          void context.workspaceState.update(PROVIDER_RECOMMENDATION_DISMISSED_KEY, true);
        }
      });
  }

  function registerTypeProviderStatusHandler(lc: LanguageClient) {
    lc.onNotification(
      NotificationType.TypeProviderStatus,
      (params: NotificationParams[typeof NotificationType.TypeProviderStatus]) => {
        const state = computeStatusBarState(params);
        typeProviderStatusBar.text = state.text;
        typeProviderStatusBar.tooltip = state.tooltip;
        typeProviderStatusBar.backgroundColor = state.warning
          ? new ThemeColor("statusBarItem.warningBackground")
          : undefined;
        log.info(
          `Type provider status: ${params.kind}${params.reason ? ` (${params.reason})` : ""}`,
        );
        // A SEPARATE line: the existing one is parsed by the E2E attestation and
        // the acceptance lane, and its shape is `kind (reason)`. The topology is
        // the answer to "which engine is actually serving", so it is recorded
        // where a log reader — or a bug report — cannot miss it.
        log.info(`Type provider topology: ${params.topology ?? "unreported"}`);
        if (params.kind !== "none") {
          startupProbe?.markTypeProviderStarted(params.kind);
        }
        maybeShowProviderRecommendation(params);
      },
    );
  }
  registerTypeProviderStatusHandler(client);

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
      log.error(
        `No heartbeat from Verter LSP for ${timeout / 1000}s — server appears frozen, restarting...`,
      );
      await restartLS(false);
    }, timeout);
  }

  function registerHeartbeatMonitor(lc: LanguageClient) {
    // Start with initial grace period (60s) — background init may take time.
    // The first heartbeat received switches to the normal 30s timeout.
    heartbeatInitialized = false;
    if (heartbeatTimer) clearTimeout(heartbeatTimer);
    heartbeatTimer = setTimeout(async () => {
      log.error(
        `No heartbeat from Verter LSP for ${HEARTBEAT_INITIAL_TIMEOUT_MS / 1000}s — server appears frozen, restarting...`,
      );
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
      startupProbe?.markReady();
      options?.onReady?.();
    });
    lc.onNotification(NotificationType.TypeProviderSyncComplete, (params: { gen: number }) => {
      log.info(`TypeProviderSyncComplete (init generation ${params.gen})`);
      options?.onTypeProviderSyncComplete?.();
    });
    lc.onNotification(NotificationType.CarrierStoreReady, (params: { carrierStoreDir: string }) => {
      log.info(`Carrier store dir reported by LSP: ${params.carrierStoreDir}`);
      // Forward to VS Code's own TS server plugin so a plain `.ts` served by
      // VS Code's TS service reads the same store the LSP publishes into.
      options?.onCarrierStoreReady?.(params.carrierStoreDir);
    });
  }

  function stopHeartbeatTimer() {
    if (heartbeatTimer) {
      clearTimeout(heartbeatTimer);
      heartbeatTimer = undefined;
    }
  }

  registerHeartbeatMonitor(client);

  // ── Extension-hosted TypeScript language service (Experiment E) ──
  // When --type-provider=extension is used, the Rust LSP sends $/verter/tsQuery
  // requests back to the extension. We lazily create the in-process TS service.
  let tsService: import("./extensionTsService").ExtensionTsService | undefined;
  function registerTsQueryHandler(lc: LanguageClient) {
    lc.onRequest(
      "$/verter/tsQuery",
      (params: { command: string; arguments: Record<string, unknown> }) => {
        if (!tsService) {
          const { ExtensionTsService } =
            require("./extensionTsService") as typeof import("./extensionTsService");
          const wsRoot = workspace.workspaceFolders?.[0]?.uri.fsPath;
          if (!wsRoot) throw new Error("No workspace root for extension TS service");
          tsService = new ExtensionTsService(wsRoot);
        }
        return tsService.handleQuery(params.command, params.arguments);
      },
    );
  }
  registerTsQueryHandler(client);

  // CSS validation diagnostics — update on document change (debounced per URI)
  const cssDiagTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const updateCssDiagnostics = async (document: TextDocument) => {
    if (!isFrameworkCarrierLanguageId(document.languageId)) return;
    try {
      const uri = document.uri.toString();
      const source = document.getText();
      if (!hasStyleBlocks(source)) {
        cssDiagnostics.delete(document.uri);
        return;
      }
      const results = await getCssService().doValidation(uri, source, document.version);
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
      if (isFrameworkCarrierLanguageId(e.document.languageId)) {
        debouncedCssDiag(e.document);
        startupProbe?.maybeTrackDocument(e.document);
      }
    }),
    workspace.onDidOpenTextDocument((document) => {
      updateCssDiagnostics(document);
      startupProbe?.maybeTrackDocument(document);
    }),
    workspace.onDidCloseTextDocument((doc) => cssDiagnostics.delete(doc.uri)),
  );

  if (startupProbe) {
    if (window.activeTextEditor) {
      startupProbe.maybeTrackDocument(window.activeTextEditor.document);
    }
    workspace.textDocuments.forEach((document) => {
      startupProbe?.maybeTrackDocument(document);
    });
    context.subscriptions.push(
      window.onDidChangeActiveTextEditor((editor) => {
        if (editor) {
          startupProbe.maybeTrackDocument(editor.document);
        }
      }),
      languages.onDidChangeDiagnostics((event) => {
        event.uris.forEach((uri) => startupProbe.maybeTrackDiagnostics(uri));
      }),
    );
  }

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
            buildServerOptions(binaryPath, rootPath, context.extensionPath, log, [
              ...sharedTsgo.lspArgs,
              ...editorTsserver.lspArgs,
            ]),
            clientOptions,
          );
          registerTypeProviderPidListener(client);
          registerHeartbeatMonitor(client);
          registerMcpListener(client);
          registerViteConfigTrustHandler(client);
          registerTypeProviderStatusHandler(client);
          tsService = undefined; // Reset TS service on restart
          registerTsQueryHandler(client);
          await client.start();
        },
        killTrackedTypeProvider,
        resetServices: () => {
          cssService?.dispose();
          cssService = undefined;
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

  // Start the language server — must be after all notification handlers and
  // listeners are registered so they're ready when the server responds.
  writeTimingMarker("client_start_begin", Date.now());
  await client.start();
  writeTimingMarker("client_start_end", Date.now());

  return {
    getClient,
    stopHeartbeatTimer,
    restart: restartLS,
  };
}

/** A live editor-owned tsgo rendezvous and its extension-host state disposer. */
interface SharedTsgoLaunch {
  /** The complete `--shared-*` pair, or an empty list when this tier is unavailable. */
  lspArgs: string[];
  /** Remove listeners and restore the relay environment. */
  dispose: () => void;
}

const NO_SHARED_TSGO: SharedTsgoLaunch = { lspArgs: [], dispose: () => {} };

/** A project-bound editor tsserver plugin receipt and its temporary-state disposer. */
interface EditorTsserverLaunch {
  /** Neutral receipt facts consumed by the LSP, or empty when this tier is unavailable. */
  lspArgs: string[];
  /** Remove the receipt after every LSP restart using it has stopped. */
  dispose: () => void;
}

const NO_EDITOR_TSSERVER: EditorTsserverLaunch = { lspArgs: [], dispose: () => {} };

/**
 * Activate Verter inside VS Code's exact editor-owned tsserver and require proof
 * that the plugin is bound to at least one current project before arming the LSP.
 */
async function establishEditorTsserverPlugin(
  typeProvider: string,
  workspaceRoot: string | undefined,
  log?: LogOutputChannel,
): Promise<EditorTsserverLaunch> {
  if (!typeProviderRoutesEditorTsserver(typeProvider) || !workspaceRoot) {
    return NO_EDITOR_TSSERVER;
  }

  const editorTypeScript = extensions.getExtension("vscode.typescript-language-features");
  if (!editorTypeScript) {
    log?.info("[editor-tsserver] not engaged - VS Code TypeScript extension is unavailable");
    return NO_EDITOR_TSSERVER;
  }

  const plan = planEditorTsserverBootstrap({ root: tmpdir() });
  try {
    const receipt = await attestEditorTsserverBootstrap(
      plan,
      {
        activate: () =>
          editorTypeScript.isActive ? editorTypeScript.exports : editorTypeScript.activate(),
        configurePlugin: (pluginId, config) =>
          commands.executeCommand("_typescript.configurePlugin", pluginId, config),
        prepareProject: () => prepareEditorTsserverConfiguredProject(workspaceRoot),
      },
      {
        acceptAttestation: (receipt) => receiptIncludesConfiguredProject(receipt, workspaceRoot),
      },
    );
    log?.info(
      `[editor-tsserver] armed: pid=${receipt.pid} projects=${JSON.stringify(receipt.projects)} ` +
        `receipt=${plan.receiptPath} (editor-owned project attested)`,
    );

    let disposed = false;
    return {
      lspArgs: plan.lspArgs,
      dispose: () => {
        if (disposed) return;
        disposed = true;
        removeEditorTsserverPlanDirectory(plan.directory, log);
      },
    };
  } catch (error) {
    removeEditorTsserverPlanDirectory(plan.directory, log);
    log?.warn(`[editor-tsserver] establish failed; continuing to managed fallback: ${error}`);
    return NO_EDITOR_TSSERVER;
  }
}

/**
 * Give Native Preview a document it activates on, so it starts a language-server
 * session for this workspace.
 *
 * Native Preview declares `onLanguage:{java,type}script[react]` and starts its
 * server for those documents; its public attestation API reports "Language
 * server is not running." until a session exists. Forcing activation does not
 * create one, so an editor whose open document is a `.vue`/`.svelte` carrier
 * attested against an extension with no server and the shared tier declined —
 * even though the engine the user wanted was installed and idle.
 *
 * A real workspace TypeScript file is preferred because it binds a real project;
 * an untitled TypeScript document is the fallback for a carrier-only workspace.
 * The document is loaded, never shown, so the user's editor layout is untouched.
 */
async function startNativePreviewLanguageServerSession(): Promise<void> {
  const exclude = "**/{node_modules,.git,dist,out,build,target,coverage,.output,.nuxt}/**";
  const [target] = await workspace.findFiles("**/*.{ts,tsx,mts,cts}", exclude, 1);
  if (target) {
    await workspace.openTextDocument(target);
    return;
  }
  await workspace.openTextDocument({ language: "typescript", content: "" });
}

/** Load one real framework carrier through VS Code's TypeScript feature without changing editors. */
async function prepareEditorTsserverConfiguredProject(workspaceRoot: string): Promise<void> {
  const exclude = "**/{node_modules,.git,dist,target}/**";
  const carrierCandidates = await workspace.findFiles("**/*.{vue,svelte}", exclude, 50);
  const selectedPath = selectEditorTsserverBootstrapCarrier(
    workspaceRoot,
    carrierCandidates.map((uri) => uri.fsPath),
  );
  const target = carrierCandidates.find((uri) => uri.fsPath === selectedPath);
  if (!target) {
    throw new Error("no workspace framework carrier can establish a configured editor project");
  }

  await workspace.openTextDocument(target);
  await commands.executeCommand("vscode.executeDocumentSymbolProvider", target);
}

/** Remove only the nonce-scoped directory this extension created under the OS temp root. */
function removeEditorTsserverPlanDirectory(directory: string, log?: LogOutputChannel): void {
  const target = resolve(directory);
  const tempRoot = resolve(tmpdir());
  if (
    !target.startsWith(`${tempRoot}${sep}`) ||
    !basename(target).startsWith("verter-editor-tsserver-")
  ) {
    log?.warn(`[editor-tsserver] refused to remove unexpected attestation path: ${target}`);
    return;
  }
  try {
    rmSync(target, { recursive: true, force: true });
  } catch (error) {
    log?.warn(`[editor-tsserver] failed to remove attestation path ${target}: ${error}`);
  }
}

/**
 * Establish the exact editor-owned Native Preview tier, fail-closed.
 *
 * Native Preview, not Verter, owns the relay and its real-tsgo child. The extension
 * temporarily selects a staged relay-shaped tsdk, requires a live advertisement and
 * a successful public-API Program attestation, then restores the user's prior global
 * tsdk setting. Any failure leaves the rendezvous unarmed for the next serving tier.
 */
async function establishSharedTsgo(
  extensionPath: string,
  workspaceRoot: string | undefined,
  typeProvider: string,
  log?: LogOutputChannel,
): Promise<SharedTsgoLaunch> {
  if (!typeProviderRoutesTsgo(typeProvider)) {
    return NO_SHARED_TSGO;
  }

  const nativePreview = extensions.getExtension<NativePreviewApi>("TypeScriptTeam.native-preview");
  if (!nativePreview) {
    log?.info("[shared-tsgo] not engaged - Native Preview extension is not installed");
    return NO_SHARED_TSGO;
  }

  try {
    // EVERY tsdk source, in order — never collapsed to a first-non-empty value.
    // A workspace `typescript.tsdk` pointing at a TS 5.x `node_modules/typescript/lib`
    // must not mask the installed Native Preview bundle behind it.
    const tsdkCandidates = nativePreviewTsdkCandidates({
      jsTsTsdkPath: workspace.getConfiguration("js/ts").get<string>("tsdk.path"),
      typescriptTsdk: workspace.getConfiguration("typescript").get<string>("tsdk"),
      nativePreviewTsdk: workspace
        .getConfiguration("typescript.native-preview")
        .get<string>("tsdk"),
      nativePreviewExtensionPath: nativePreview.extensionPath,
    });
    const plan = planSharedTsgo({
      extensionPath,
      controlDirRoot: tmpdir(),
      env: process.env,
      tsdkCandidates,
      workspaceRoot,
    });
    // Previous sessions' rendezvous dirs are swept regardless of the plan's outcome —
    // staging leaked one per session and they accumulated indefinitely.
    sweepOrphanedControlDirs(plan.engaged ? plan.sessionKey : undefined, log);
    if (!plan.engaged) {
      log?.info(`[shared-tsgo] not engaged — ${plan.reason}`);
      return NO_SHARED_TSGO;
    }

    // Stage the npm-shaped tsdk tree Native Preview's validator accepts, with the
    // relay bytes at the engine path it stats. The editor performs the process spawn;
    // Verter supplies only inherited rendezvous metadata.
    const staged = prepareEditorTsdk({
      shimPath: plan.shimPath,
      controlDir: plan.controlDir,
    });
    const restoreEnvironment = installProcessEnvironment(buildRelayEditorEnv(plan));
    const nativePreviewConfig = workspace.getConfiguration("typescript.native-preview");
    const controller = new NativePreviewRelayController({
      stagedTsdk: staged.dir,
      isExtensionActive: () => nativePreview.isActive,
      activate: async () => nativePreview.activate(),
      restart: async () => {
        await commands.executeCommand("typescript.native-preview.restart");
      },
      readGlobalTsdk: () => nativePreviewConfig.inspect<string>("tsdk")?.globalValue,
      writeGlobalTsdk: async (value) => {
        await nativePreviewConfig.update("tsdk", value, ConfigurationTarget.Global);
      },
      startSession: () => startNativePreviewLanguageServerSession(),
      hasAdvertisement: () => {
        try {
          return readdirSync(plan.controlDir).some(isShimAdvertisement);
        } catch {
          return false;
        }
      },
      onBackgroundError: (error) => {
        log?.warn(`[shared-tsgo] Native Preview re-attach failed: ${error}`);
      },
    });

    try {
      await controller.establish();
    } catch (error) {
      controller.dispose();
      restoreEnvironment();
      throw error;
    }
    log?.info(
      `[shared-tsgo] armed: shim=${plan.shimPath} realTsgo=${plan.realTsgo} ` +
        `realTsgoSource=${plan.realTsgoSource} ` +
        `controlDir=${plan.controlDir} (SHARED editor-attach attested against Native Preview's current Program)`,
    );

    let disposed = false;
    return {
      lspArgs: plan.lspArgs,
      dispose: () => {
        if (disposed) return;
        disposed = true;
        controller.dispose();
        restoreEnvironment();
        removeControlDir(plan.controlDir, log);
      },
    };
  } catch (err) {
    log?.warn(`[shared-tsgo] establish failed; continuing to the next serving tier: ${err}`);
    return NO_SHARED_TSGO;
  }
}

/** Remove one `verter-shared-<key>` rendezvous dir this extension created. */
function removeControlDir(directory: string, log?: LogOutputChannel): void {
  const target = resolve(directory);
  const tempRoot = resolve(tmpdir());
  if (!target.startsWith(`${tempRoot}${sep}`) || !isVerterSharedControlDirName(basename(target))) {
    log?.warn(`[shared-tsgo] refused to remove unexpected rendezvous path: ${target}`);
    return;
  }
  try {
    rmSync(target, { recursive: true, force: true });
  } catch (error) {
    log?.warn(`[shared-tsgo] failed to remove rendezvous path ${target}: ${error}`);
  }
}

/** How long an unclaimed rendezvous dir may sit before this session sweeps it. */
const ORPHANED_CONTROL_DIR_MAX_AGE_MS = 3_600_000;

/**
 * Delete stale `verter-shared-*` rendezvous dirs left by previous sessions.
 *
 * Every session created one and none were ever removed. The live session's own
 * key and anything younger than {@link ORPHANED_CONTROL_DIR_MAX_AGE_MS} are
 * skipped so a concurrent editor window's rendezvous survives.
 */
function sweepOrphanedControlDirs(currentSessionKey: string | undefined, log?: LogOutputChannel) {
  const tempRoot = resolve(tmpdir());
  let entries: string[];
  try {
    entries = readdirSync(tempRoot);
  } catch {
    return;
  }
  const stale = orphanedControlDirs({
    entries,
    currentSessionKey,
    now: Date.now(),
    maxAgeMs: ORPHANED_CONTROL_DIR_MAX_AGE_MS,
    modifiedAt: (name) => {
      try {
        return statSync(join(tempRoot, name)).mtimeMs;
      } catch {
        return undefined;
      }
    },
  });
  let removed = 0;
  for (const name of stale) {
    try {
      rmSync(join(tempRoot, name), { recursive: true, force: true });
      removed += 1;
    } catch {
      // A dir held open by a live peer session is left alone.
    }
  }
  if (removed > 0) {
    log?.info(`[shared-tsgo] swept ${removed} orphaned rendezvous dir(s) from ${tempRoot}`);
  }
}

/** Install child-process environment values and return an exact restoration closure. */
function installProcessEnvironment(values: Record<string, string>): () => void {
  const previous = new Map<string, string | undefined>();
  for (const [key, value] of Object.entries(values)) {
    previous.set(key, process.env[key]);
    process.env[key] = value;
  }

  let restored = false;
  return () => {
    if (restored) return;
    restored = true;
    for (const [key, value] of previous) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  };
}

function buildServerOptions(
  binaryPath: string,
  rootPath: string | undefined,
  extensionPath: string,
  log?: LogOutputChannel,
  sharedLspArgs: string[] = [],
): ServerOptions {
  const logLevel = workspace.getConfiguration("verter.server").get<string>("logLevel", "info");
  const verterConfig = workspace.getConfiguration("verter");
  // E2E tests can override the type provider via environment variable
  const typeProvider =
    readE2eEnv("TYPE_PROVIDER") || verterConfig.get<string>("typeProvider", "auto");
  const userTsdk = verterConfig.get<string>("typescript.tsdk", "");
  // Always pass --tsdk: user setting → bundled TypeScript (fallback for pnpm strict mode etc.)
  const bundledTsdk = join(extensionPath, "node_modules", "typescript", "lib");
  const tsdk = userTsdk || bundledTsdk;

  const mcpEnabled = verterConfig.get<boolean>("mcp.enabled", true);
  const mcpLintPreset = verterConfig.get<string>("mcp.lintPreset", "recommended");

  const args: string[] = [];
  args.push(`--type-provider=${typeProvider}`);
  args.push(`--tsdk=${tsdk}`);
  args.push(`--plugin-path=${join(extensionPath, "node_modules")}`);
  if (mcpEnabled) {
    args.push(`--mcp-port=0`);
    args.push(`--mcp-lint-preset=${mcpLintPreset}`);
  }
  // Pass only attested editor-serving facts. These are --prefixed flags, so they
  // precede the positional workspace-root argument the LSP parses last.
  args.push(...sharedLspArgs);
  if (rootPath) args.push(rootPath);

  log?.info(
    `[buildServerOptions] typeProvider=${typeProvider}, tsdk=${tsdk}${userTsdk ? "" : " (bundled)"}, args=${JSON.stringify(args)}`,
  );

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

function addDidChangeTextDocumentListener(getClient: GetClient): Disposable {
  return workspace.onDidChangeTextDocument((e) => {
    // Only forward TS/JS changes — .vue changes are handled by the LSP's own did_change.
    // Sending .vue here would cause redundant notifications and TSGO flooding.
    if (e.document.languageId !== "typescript" && e.document.languageId !== "javascript") {
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
  context: ExtensionContext,
  ensureLanguageServerStarted: () => Promise<unknown>,
) {
  context.subscriptions.push(
    commands.registerTextEditorCommand("verter.showCompiledCodeToSide", async (editor) => {
      if (!isFrameworkCarrierLanguageId(editor?.document?.languageId)) {
        window.showInformationMessage("Not a component file");
        return;
      }

      window.withProgress(
        { location: ProgressLocation.Window, title: "Compiling..." },
        async () => {
          await ensureLanguageServerStarted();
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
function addWriteVirtualFilesCommand(
  context: ExtensionContext,
  ensureLanguageServerStarted: () => Promise<unknown>,
) {
  context.subscriptions.push(
    commands.registerTextEditorCommand("verter.writeVirtualFiles", async (editor) => {
      if (!isFrameworkCarrierLanguageId(editor?.document?.languageId)) {
        window.showInformationMessage("Not a component file");
        return;
      }

      window.withProgress(
        { location: ProgressLocation.Window, title: "Compiling..." },
        async () => {
          await ensureLanguageServerStarted();
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

function addShowStatisticsCommand(
  context: ExtensionContext,
  log: LogOutputChannel,
  ensureLanguageServerStarted: () => Promise<unknown>,
  getClient: GetClient,
) {
  const channel = window.createOutputChannel("Verter Statistics");

  context.subscriptions.push(
    channel,
    commands.registerCommand("verter.showStatistics", async () => {
      try {
        await ensureLanguageServerStarted();
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

  // Track last active framework-carrier file URI so the sidebar persists when
  // virtual files or non-carrier files are focused (any carrier: .vue/.svelte).
  let lastCarrierFileUri: string | undefined;
  const getLastCarrierUri = () => lastCarrierFileUri;
  const updateLastCarrierFile = () => {
    const editor = window.activeTextEditor;
    if (isFrameworkCarrierLanguageId(editor?.document?.languageId)) {
      lastCarrierFileUri = editor!.document.uri.toString();
    }
  };
  updateLastCarrierFile();
  context.subscriptions.push(window.onDidChangeActiveTextEditor(updateLastCarrierFile));

  // Track whether the active editor is a framework-carrier file (.vue/.svelte).
  const updateHasActiveCarrierFile = () => {
    const isCarrier = isFrameworkCarrierLanguageId(window.activeTextEditor?.document?.languageId);
    commands.executeCommand("setContext", "verter.hasActiveCarrierFile", isCarrier);
  };
  updateHasActiveCarrierFile();
  context.subscriptions.push(window.onDidChangeActiveTextEditor(updateHasActiveCarrierFile));

  // Create content provider for virtual files (no disk writes — uses verter-virtual:// scheme)
  const contentProvider = new VirtualFileContentProvider();
  context.subscriptions.push(
    workspace.registerTextDocumentContentProvider(
      VirtualFileContentProvider.scheme,
      contentProvider,
    ),
    contentProvider,
  );

  const virtualFilesProvider = new UnifiedVirtualFilesProvider(
    getClient,
    contentProvider,
    getLastCarrierUri,
  );
  const componentTreeProvider = new ComponentTreeProvider(getClient, getLastCarrierUri);
  const routeTreeProvider = new RouteTreeProvider(getClient, getLastCarrierUri);
  const analysisProvider = new AnalysisTreeProvider(getClient, getLastCarrierUri);
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
      // D113: bridge custom-method LSP requests through VS Code commands
      // so e2e tests can drive `getComponentMeta` / `getComponentMetaSurface`
      // / `getComponentMetaTypeExpansion` without holding the language
      // client themselves. Tests call e.g.
      // `commands.executeCommand("verter._getComponentMeta", { uri })`.
      commands.registerCommand("verter._getComponentMeta", (params: { uri: string }) =>
        getClient().sendRequest(RequestType.GetComponentMeta, params),
      ),
      commands.registerCommand("verter._getComponentMetaSurface", (params: { uri: string }) =>
        getClient().sendRequest(RequestType.GetComponentMetaSurface, params),
      ),
      commands.registerCommand(
        "verter._getComponentMetaTypeExpansion",
        (params: { handleBytes: number[]; depth?: number }) =>
          getClient().sendRequest(RequestType.GetComponentMetaTypeExpansion, params),
      ),
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
    window.createTreeView("verterRoutes", {
      treeDataProvider: routeTreeProvider,
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
    commands.registerCommand("verter.refreshRoutes", () => {
      routeTreeProvider.refresh();
    }),
    commands.registerCommand("verter.openRouteComponent", async (filePath: string) => {
      try {
        const doc = await workspace.openTextDocument(Uri.file(filePath));
        await window.showTextDocument(doc);
      } catch {
        // File might not exist
      }
    }),
    commands.registerCommand("verter.showSourceMapVisualization", async () => {
      const sourceUri = getLastCarrierUri();
      if (!sourceUri) {
        window.showInformationMessage("No active component file");
        return;
      }

      // Get source code from the open document or fall back to reading from disk
      const vueDoc = workspace.textDocuments.find((d) => d.uri.toString() === sourceUri);
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
        const sourceUri = item.sourceUri || getLastCarrierUri();
        if (!sourceUri) {
          window.showInformationMessage("No active component file");
          return;
        }

        const vueDoc = workspace.textDocuments.find((d) => d.uri.toString() === sourceUri);
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

function addNodeModulesChangedListener(getClient: GetClient): Disposable {
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
  const workspaceFoldersListener = workspace.onDidChangeWorkspaceFolders((e) => {
    e.removed.forEach((folder) => {
      watchers.get(folder.uri.fsPath)?.dispose();
      watchers.delete(folder.uri.fsPath);
    });
    e.added.forEach(watchFolder);
  });

  return {
    dispose() {
      workspaceFoldersListener.dispose();
      watchers.forEach((watcher) => watcher.dispose());
      watchers.clear();
    },
  };
}

function addViteConfigChangedListener(getClient: GetClient): Disposable {
  const pattern = "**/vite.config.{ts,js,mjs,cjs,mts,cts}";
  const watcher = workspace.createFileSystemWatcher(pattern);

  const send = (uri: Uri, type: "create" | "update" | "delete") => {
    getClient().sendNotification(NotificationType.OnFileChanged, {
      type,
      uri: uri.toString(),
    });
  };

  watcher.onDidChange((e) => send(e, "update"));
  watcher.onDidCreate((e) => send(e, "create"));
  watcher.onDidDelete((e) => send(e, "delete"));

  return {
    dispose() {
      watcher.dispose();
    },
  };
}
