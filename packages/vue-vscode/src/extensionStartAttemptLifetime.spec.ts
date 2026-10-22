import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * One language-server start attempt owns everything it creates — event
 * subscriptions, the type-provider status bar item, the heartbeat watchdog and
 * the recovery of every server it starts — and leaves nothing behind when its
 * owner disposes it.
 *
 * Without that ownership a server that could not launch produced an
 * ever-growing row of status bar items, an uncancellable restart timer firing
 * against a dead client, and a fresh attempt with a fresh retry budget for
 * every document opened afterwards: a restart storm with no visible end.
 *
 * The assertions are on DISPOSAL (`item.dispose()` calls, armed timer count),
 * OBSERVED PROCESS STARTS and STOPS, and the state the status bar item is left
 * in — the stopped state is the user-visible outcome under test.
 */

const mocks = vi.hoisted(() => {
  const statusBarItems: { disposed: boolean }[] = [];
  const diagnosticCollections: { disposed: boolean }[] = [];
  const errorMessages: string[] = [];
  const warningMessages: string[] = [];
  const state = {
    startShouldFail: true,
    startCalls: 0,
    stopCalls: 0,
    createdClientOptions: [] as unknown[],
    /** Park `start()` so a test can dispose the extension mid-start. */
    holdStart: false,
    /** When set, the editor TypeScript extension exists and its activation parks on this. */
    editorTypeScriptActivate: undefined as (() => Promise<unknown>) | undefined,
    releaseStart: undefined as (() => void) | undefined,
    configurePluginHold: false,
    releaseConfigurePlugin: undefined as (() => void) | undefined,
    configurePluginConfigs: [] as unknown[],
    closeListeners: [] as Array<(document: unknown) => void>,
    configurationListeners: [] as Array<(event: unknown) => void>,
    infoMessages: [] as string[],
    createdClients: [] as Array<{
      handlers: Map<string, Array<(params: unknown) => void>>;
    }>,
  };
  const configurationValues: Record<string, unknown> = {};
  return {
    statusBarItems,
    diagnosticCollections,
    errorMessages,
    warningMessages,
    state,
    configurationValues,
  };
});

vi.mock("vscode", () => {
  const subscribe = () => ({ dispose: () => {} });
  const disposable = () => ({ dispose: () => {} });
  return {
    window: {
      createStatusBarItem: () => {
        const item = {
          disposed: false,
          text: "",
          tooltip: "",
          command: "",
          backgroundColor: undefined as unknown,
          show() {},
          hide() {},
          dispose() {
            this.disposed = true;
          },
        };
        mocks.statusBarItems.push(item);
        return item;
      },
      createOutputChannel: () => ({
        info() {},
        warn() {},
        error() {},
        trace() {},
        debug() {},
        append() {},
        appendLine() {},
        show() {},
        dispose() {},
      }),
      onDidChangeActiveTextEditor: subscribe,
      activeTextEditor: undefined,
      showInformationMessage: async (message: string) => {
        mocks.state.infoMessages.push(message);
        return undefined;
      },
      showWarningMessage: async (message: string) => {
        mocks.warningMessages.push(message);
        return undefined;
      },
      showErrorMessage: async (message: string) => {
        mocks.errorMessages.push(message);
        return undefined;
      },
      showTextDocument: async () => undefined,
      showQuickPick: async () => undefined,
      withProgress: async (_options: unknown, task: (...args: unknown[]) => unknown) =>
        task({ report() {} }, { isCancellationRequested: false, onCancellationRequested() {} }),
      createTextEditorDecorationType: () => disposable(),
      registerWebviewPanelSerializer: () => disposable(),
      createWebviewPanel: () => ({ dispose() {} }),
      registerTreeDataProvider: () => disposable(),
      createTreeView: () => ({ dispose() {} }),
      visibleTextEditors: [] as unknown[],
    },
    workspace: {
      workspaceFolders: undefined,
      textDocuments: [] as unknown[],
      getConfiguration: (section: string) => ({
        get: (key: string, fallback?: unknown) =>
          mocks.configurationValues[`${section}.${key}`] ?? fallback,
        inspect: () => undefined,
        update: async () => {},
      }),
      onDidOpenTextDocument: subscribe,
      onDidChangeTextDocument: subscribe,
      onDidCloseTextDocument: (listener: (document: unknown) => void) => {
        mocks.state.closeListeners.push(listener);
        return {
          dispose() {
            const index = mocks.state.closeListeners.indexOf(listener);
            if (index >= 0) {
              mocks.state.closeListeners.splice(index, 1);
            }
          },
        };
      },
      onDidSaveTextDocument: subscribe,
      onDidChangeConfiguration: (listener: (event: unknown) => void) => {
        mocks.state.configurationListeners.push(listener);
        return {
          dispose() {
            const index = mocks.state.configurationListeners.indexOf(listener);
            if (index >= 0) {
              mocks.state.configurationListeners.splice(index, 1);
            }
          },
        };
      },
      onDidChangeWorkspaceFolders: subscribe,
      registerTextDocumentContentProvider: () => disposable(),
      createFileSystemWatcher: () => ({
        onDidCreate: subscribe,
        onDidChange: subscribe,
        onDidDelete: subscribe,
        dispose() {},
      }),
      findFiles: async () => [],
      openTextDocument: async () => ({}),
      asRelativePath: (value: string) => value,
    },
    commands: {
      registerCommand: () => disposable(),
      registerTextEditorCommand: () => disposable(),
      executeCommand: async (command: string, ...args: unknown[]) => {
        if (command === "_typescript.configurePlugin") {
          mocks.state.configurePluginConfigs.push(args[1]);
          if (mocks.state.configurePluginHold) {
            await new Promise<void>((resolve) => {
              mocks.state.releaseConfigurePlugin = resolve;
            });
          }
        }
        return undefined;
      },
    },
    languages: {
      createDiagnosticCollection: () => {
        const collection = {
          disposed: false,
          set() {},
          delete() {},
          clear() {},
          dispose() {
            this.disposed = true;
          },
        };
        mocks.diagnosticCollections.push(collection);
        return collection;
      },
      onDidChangeDiagnostics: subscribe,
      registerCodeActionsProvider: () => disposable(),
      registerCodeLensProvider: () => disposable(),
      registerHoverProvider: () => disposable(),
    },
    extensions: {
      getExtension: (id: string) =>
        id === "vscode.typescript-language-features" && mocks.state.editorTypeScriptActivate
          ? { isActive: false, exports: {}, activate: mocks.state.editorTypeScriptActivate }
          : undefined,
      all: [] as unknown[],
    },
    lm: { registerMcpServerDefinitionProvider: () => disposable() },
    Uri: {
      parse: (value: string) => ({ toString: () => value, fsPath: value, scheme: "file" }),
      file: (value: string) => ({ toString: () => value, fsPath: value, scheme: "file" }),
      joinPath: (base: unknown, ...parts: string[]) => ({ toString: () => parts.join("/") }),
    },
    StatusBarAlignment: { Left: 1, Right: 2 },
    ProgressLocation: { SourceControl: 1, Window: 10, Notification: 15 },
    ViewColumn: { Active: -1, Beside: -2, One: 1 },
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
    ConfigurationTarget: { Global: 1, Workspace: 2, WorkspaceFolder: 3 },
    TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
    ThemeColor: class {
      constructor(public id: string) {}
    },
    ThemeIcon: class {
      constructor(public id: string) {}
    },
    McpHttpServerDefinition: class {
      constructor(
        public label: string,
        public uri: unknown,
      ) {}
    },
    Diagnostic: class {
      constructor(
        public range: unknown,
        public message: string,
        public severity?: number,
      ) {}
    },
    Range: class {
      constructor(
        public start: unknown,
        public end: unknown,
      ) {}
    },
    Position: class {
      constructor(
        public line: number,
        public character: number,
      ) {}
    },
    Location: class {
      constructor(
        public uri: unknown,
        public range: unknown,
      ) {}
    },
    TreeItem: class {
      constructor(
        public label: unknown,
        public collapsibleState?: number,
      ) {}
    },
    MarkdownString: class {
      value = "";
      appendMarkdown() {
        return this;
      }
      appendText() {
        return this;
      }
      appendCodeblock() {
        return this;
      }
    },
    EventEmitter: class {
      event = () => ({ dispose: () => {} });
      fire() {}
      dispose() {}
    },
    Disposable: class {
      constructor(private readonly fn: () => void) {}
      dispose() {
        this.fn();
      }
    },
    RelativePattern: class {
      constructor(
        public base: unknown,
        public pattern: string,
      ) {}
    },
    CodeActionKind: { QuickFix: { value: "quickfix" }, SourceOrganizeImports: { value: "source" } },
    DecorationRangeBehavior: { OpenOpen: 0, ClosedClosed: 1, OpenClosed: 2, ClosedOpen: 3 },
    OverviewRulerLane: { Left: 1, Center: 2, Right: 4, Full: 7 },
  };
});

vi.mock("vscode-languageclient/node", () => ({
  LanguageClient: class {
    handlers = new Map<string, Array<(params: unknown) => void>>();
    constructor(_id: string, _name: string, _serverOptions: unknown, clientOptions: unknown) {
      mocks.state.createdClientOptions.push(clientOptions);
      mocks.state.createdClients.push(this);
    }
    protocol2CodeConverter = {};
    onNotification(type: unknown, handler: (params: unknown) => void) {
      const key = String(type);
      const list = this.handlers.get(key) ?? [];
      list.push(handler);
      this.handlers.set(key, list);
      return { dispose: () => {} };
    }
    onRequest() {
      return { dispose: () => {} };
    }
    onDidChangeState() {
      return { dispose: () => {} };
    }
    sendNotification() {
      return Promise.resolve();
    }
    async start() {
      mocks.state.startCalls += 1;
      if (mocks.state.startShouldFail) {
        throw new Error("verter-lsp exited before the initialize handshake");
      }
      if (mocks.state.holdStart) {
        await new Promise<void>((resolve) => {
          mocks.state.releaseStart = resolve;
        });
      }
    }
    async stop() {
      mocks.state.stopCalls += 1;
    }
  },
  State: { Stopped: 1, Running: 2, Starting: 3 },
  ErrorAction: { Continue: 1, Shutdown: 2 },
  CloseAction: { DoNotRestart: 1, Restart: 2 },
  TransportKind: { stdio: 0, ipc: 1, pipe: 2, socket: 3 },
  RevealOutputChannelOn: { Info: 1, Warn: 2, Error: 3, Never: 4 },
}));

import { window, workspace } from "vscode";
import { activate, activateVueLanguageServer, deactivate } from "./extension";
import { DEFAULT_RESTART_POLICY } from "./restart";
import { StartAttemptScope } from "./startAttemptScope";

type ActivateParams = Parameters<typeof activateVueLanguageServer>;

/** Start one language-server attempt owned by `lifetime`, as the activation root does. */
function startAttempt(
  context: ActivateParams[0],
  lifetime: StartAttemptScope = new StartAttemptScope(),
) {
  return activateVueLanguageServer(context, log, undefined, { lifetime });
}

const log = {
  info() {},
  warn() {},
  error() {},
  trace() {},
  debug() {},
  show() {},
  append() {},
  appendLine() {},
  dispose() {},
} as unknown as ActivateParams[1];

function makeContext() {
  return {
    extensionPath: "/verter/packages/vue-vscode",
    subscriptions: [] as { dispose(): unknown }[],
    workspaceState: {
      get: (_key: string, fallback?: unknown) => fallback,
      update: async () => {},
    },
    globalState: {
      get: (_key: string, fallback?: unknown) => fallback,
      update: async () => {},
    },
  } as unknown as ActivateParams[0] & { subscriptions: { dispose(): unknown }[] };
}

function resetHostRuntimeMocks() {
  mocks.statusBarItems.length = 0;
  mocks.diagnosticCollections.length = 0;
  mocks.errorMessages.length = 0;
  mocks.warningMessages.length = 0;
  mocks.state.startCalls = 0;
  mocks.state.stopCalls = 0;
  mocks.state.holdStart = false;
  mocks.state.editorTypeScriptActivate = undefined;
  mocks.state.releaseStart = undefined;
  mocks.state.configurePluginHold = false;
  mocks.state.releaseConfigurePlugin = undefined;
  mocks.state.configurePluginConfigs.length = 0;
  mocks.state.closeListeners.length = 0;
  mocks.state.configurationListeners.length = 0;
  mocks.state.infoMessages.length = 0;
  mocks.state.createdClients.length = 0;
  mocks.state.createdClientOptions.length = 0;
  for (const key of Object.keys(mocks.configurationValues)) {
    delete mocks.configurationValues[key];
  }
}

const liveStatusBarItems = () => mocks.statusBarItems.filter((item) => !item.disposed).length;

type MockStatusBarItem = { disposed: boolean; text: string; command: string };
const liveStatusBarItem = () =>
  (mocks.statusBarItems as MockStatusBarItem[]).find((item) => !item.disposed);

type ErrorHandlerLike = {
  error(error: Error, message: unknown, count: number | undefined): { action: number };
  closed(): { action: number; handled?: boolean };
};
/** The error handler the extension installed on the `index`-th client it created. */
const errorHandlerOf = (index: number) =>
  (mocks.state.createdClientOptions[index] as { errorHandler?: ErrorHandlerLike }).errorHandler!;

describe("language server start attempt lifetime", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = true;
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("recovers a server that cannot start within the budget, then shows it stopped and releases everything on disposal", async () => {
    const context = makeContext();
    const lifetime = new StartAttemptScope();

    // The attempt survives its failed first start: recovery, not the next
    // opened document, decides whether another process is spawned.
    await startAttempt(context, lifetime);
    expect(mocks.state.startCalls).toBe(1);

    // Far more virtual time than the whole budget needs (2+4+8+16+32s).
    await vi.advanceTimersByTimeAsync(900_000);

    expect(mocks.state.startCalls).toBe(1 + DEFAULT_RESTART_POLICY.maxAutomaticRestarts);
    // ONE status bar item across every retry, now in the stopped state that
    // restarts the server when clicked.
    expect(liveStatusBarItems()).toBe(1);
    expect(liveStatusBarItem()?.text).toMatch(/stopped/);
    expect(liveStatusBarItem()?.command).toBe("verter.restartLanguageServer");
    expect(mocks.errorMessages.some((m) => /Restart Language Server/.test(m))).toBe(true);
    // Quiescent: nothing is armed after the give-up.
    expect(vi.getTimerCount()).toBe(0);

    lifetime.dispose();

    expect(liveStatusBarItems()).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(mocks.statusBarItems.length).toBe(1);
    expect(mocks.diagnosticCollections.every((collection) => collection.disposed)).toBe(true);
  });

  it("cancels the heartbeat watchdog when the extension disposes a started server", async () => {
    mocks.state.startShouldFail = false;
    const lifetime = new StartAttemptScope();

    await startAttempt(makeContext(), lifetime);
    // A live attempt is armed: the watchdog timer and the status bar item exist.
    expect(vi.getTimerCount()).toBeGreaterThan(0);
    expect(liveStatusBarItems()).toBe(1);

    lifetime.dispose();

    expect(vi.getTimerCount()).toBe(0);
    expect(liveStatusBarItems()).toBe(0);
  });

  // @ai-generated - Verifies that the real extension restart path re-reads init-only settings.
  it("rebuilds initialization options from current settings when restarting", async () => {
    mocks.state.startShouldFail = false;
    const context = makeContext();
    const server = await startAttempt(context);

    type ClientOptions = {
      initializationOptions: {
        analysis: { enabled: boolean };
        hover: { nativeSemantics: boolean; provenance: boolean };
      };
    };
    const initial = mocks.state.createdClientOptions[0] as ClientOptions;
    expect(initial.initializationOptions.analysis.enabled).toBe(false);
    expect(initial.initializationOptions.hover).toEqual({
      nativeSemantics: false,
      provenance: false,
    });

    mocks.configurationValues["verter.analysis.enabled"] = true;
    mocks.configurationValues["verter.hover.nativeSemantics"] = true;
    mocks.configurationValues["verter.hover.provenance"] = true;
    await server.restart(false);

    expect(mocks.state.createdClientOptions).toHaveLength(2);
    const restarted = mocks.state.createdClientOptions[1] as ClientOptions;
    expect(restarted).not.toBe(initial);
    expect(restarted.initializationOptions.analysis.enabled).toBe(true);
    expect(restarted.initializationOptions.hover).toEqual({
      nativeSemantics: true,
      provenance: true,
    });
    expect(initial.initializationOptions.analysis.enabled).toBe(false);
    expect(initial.initializationOptions.hover.nativeSemantics).toBe(false);
  });
});

/**
 * The heartbeat watchdog restarts a frozen server. Against a server that CANNOT
 * come back, that recovery used to have no counter, no delay and no ceiling: the
 * monitor re-armed a fresh 60s timer before `client.start()` was even awaited, so
 * a start that threw still scheduled the next restart — a permanent background
 * process storm at a fixed cadence with no path to quiescence.
 *
 * The assertions are on OBSERVED PROCESS STARTS, so a change that merely silences
 * the log cannot pass them.
 */
describe("automatic restart bounding", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.statusBarItems.length = 0;
    mocks.diagnosticCollections.length = 0;
    mocks.errorMessages.length = 0;
    mocks.state.startShouldFail = false;
    mocks.state.startCalls = 0;
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("stops restarting a server that can never start, and says how to retry", async () => {
    await startAttempt(makeContext());
    expect(mocks.state.startCalls).toBe(1);

    // From here every start throws. Let far more virtual time pass than the
    // whole retry budget needs (60s watchdog + 2+4+8+16+32s of backoff), with
    // enough headroom that a WIDER budget would visibly spend it here.
    mocks.state.startShouldFail = true;
    await vi.advanceTimersByTimeAsync(900_000);

    expect(mocks.state.startCalls).toBe(1 + DEFAULT_RESTART_POLICY.maxAutomaticRestarts);
    expect(mocks.errorMessages.some((m) => /Restart Language Server/.test(m))).toBe(true);

    // Quiescence: nothing re-arms after the give-up.
    const settled = mocks.state.startCalls;
    await vi.advanceTimersByTimeAsync(600_000);
    expect(mocks.state.startCalls).toBe(settled);
  });

  it("still restarts a frozen server that can come back", async () => {
    await startAttempt(makeContext());
    expect(mocks.state.startCalls).toBe(1);

    // One watchdog trip (60s) plus the first backoff (2s).
    await vi.advanceTimersByTimeAsync(63_000);

    expect(mocks.state.startCalls).toBe(2);
    expect(mocks.errorMessages).toEqual([]);
  });
});

/**
 * Deactivation while a recovery is mid-flight.
 *
 * The user disables the extension or reloads the window between the watchdog
 * trip and the restart it scheduled. Recovery outliving the extension instance
 * that owns it means a language server process nobody owns and nothing will
 * ever shut down, so the assertions are on OBSERVED PROCESS STARTS and STOPS.
 */
describe("disposal during an in-flight recovery", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = false;
  });

  afterEach(() => {
    mocks.state.releaseStart?.();
    mocks.state.holdStart = false;
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("starts no server when disposal lands inside the restart backoff", async () => {
    const lifetime = new StartAttemptScope();
    await startAttempt(makeContext(), lifetime);
    expect(mocks.state.startCalls).toBe(1);

    // The watchdog trips at 60s and recovery enters its 2s backoff.
    await vi.advanceTimersByTimeAsync(61_000);
    expect(mocks.state.startCalls).toBe(1);

    // The user disables the extension / reloads the window mid-backoff.
    lifetime.dispose();

    // The backoff delay expires afterwards, and every later window passes.
    await vi.advanceTimersByTimeAsync(600_000);

    expect(mocks.state.startCalls).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shuts down a server whose start completed after disposal", async () => {
    const lifetime = new StartAttemptScope();
    await startAttempt(makeContext(), lifetime);
    expect(mocks.state.startCalls).toBe(1);

    // The next start parks, so disposal lands while the restart is executing.
    mocks.state.holdStart = true;
    await vi.advanceTimersByTimeAsync(63_000);
    expect(mocks.state.startCalls).toBe(2);
    expect(mocks.state.releaseStart).toBeDefined();
    const stopsBeforeDisposal = mocks.state.stopCalls;

    lifetime.dispose();
    // The process finishes coming up — for an owner that no longer exists.
    mocks.state.releaseStart?.();
    await vi.advanceTimersByTimeAsync(600_000);

    // The orphan is shut down, and no watchdog is armed over it.
    expect(mocks.state.stopCalls).toBe(stopsBeforeDisposal + 1);
    expect(mocks.state.startCalls).toBe(2);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shuts down an initial server whose start completed after disposal", async () => {
    const lifetime = new StartAttemptScope();

    // The FIRST start parks, so deactivation lands while the initial
    // language-server start is still pending: the runtime was never
    // published, so deactivate() had no client to stop and disposal of the
    // attempt is all the ownership this start ever had.
    mocks.state.holdStart = true;
    const activating = startAttempt(makeContext(), lifetime);
    for (let i = 0; i < 10_000 && !mocks.state.releaseStart; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.releaseStart).toBeDefined();
    expect(mocks.state.startCalls).toBe(1);
    expect(mocks.state.stopCalls).toBe(0);

    lifetime.dispose();
    expect(vi.getTimerCount()).toBe(0);

    // The process finishes coming up — for an owner that no longer exists.
    mocks.state.releaseStart?.();
    await activating;
    await vi.advanceTimersByTimeAsync(600_000);

    // The orphan is shut down exactly once, and no watchdog is armed over it.
    expect(mocks.state.stopCalls).toBe(1);
    expect(mocks.state.startCalls).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("disposal during provider setup", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = false;
  });

  afterEach(() => {
    (workspace as { workspaceFolders: unknown }).workspaceFolders = undefined;
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("constructs no client when the attempt is disposed while the editor provider is being established", async () => {
    (workspace as { workspaceFolders: unknown }).workspaceFolders = [{ uri: { fsPath: "/proj" } }];
    mocks.configurationValues["verter.typeProvider"] = "editor-tsserver";
    // The editor TypeScript extension never finishes activating.
    mocks.state.editorTypeScriptActivate = () => new Promise<never>(() => {});
    const lifetime = new StartAttemptScope();

    const activating = startAttempt(makeContext(), lifetime);
    const outcome = activating.then(
      () => undefined,
      (error: unknown) => error,
    );
    await vi.advanceTimersByTimeAsync(100);
    lifetime.dispose();
    // The bounded provider operation gives up; the start must not carry on past it.
    await vi.advanceTimersByTimeAsync(600_000);

    expect(await outcome).toMatchObject({ name: "StartAbandonedError" });
    expect(mocks.state.createdClients).toHaveLength(0);
    expect(mocks.state.startCalls).toBe(0);
  });
});

describe("language server ownership through runtime publication", () => {
  const vueDocument = {
    languageId: "vue",
    version: 1,
    getText: () => "",
    uri: {
      scheme: "file",
      fsPath: "/proj/App.vue",
      toString: () => "file:///proj/App.vue",
    },
  };

  beforeEach(() => {
    vi.useFakeTimers({
      toFake: ["setTimeout", "setInterval", "clearTimeout", "clearInterval"],
    });
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = false;
    mocks.configurationValues["verter.mcp.enabled"] = false;
    (workspace as { textDocuments: unknown[] }).textDocuments.length = 0;
    (workspace as { textDocuments: unknown[] }).textDocuments.push(vueDocument);
    (window as { activeTextEditor?: { document: unknown } }).activeTextEditor = {
      document: vueDocument,
    };
  });

  afterEach(() => {
    try {
      deactivate();
    } catch {
      // Root may already be empty.
    }
    (workspace as { textDocuments: unknown[] }).textDocuments.length = 0;
    (window as { activeTextEditor?: { document: unknown } }).activeTextEditor = undefined;
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  async function waitForHeldStart(): Promise<void> {
    for (let i = 0; i < 10_000 && !mocks.state.releaseStart; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.releaseStart).toBeDefined();
  }

  it("stops a started client that deactivates before runtime publication", async () => {
    const context = makeContext();
    mocks.state.holdStart = true;

    const activating = activate(context);
    await waitForHeldStart();
    await activating;
    expect(mocks.state.startCalls).toBe(1);
    expect(mocks.state.stopCalls).toBe(0);

    mocks.state.releaseStart?.();
    let watchdogArmed = false;
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
      if (vi.getTimerCount() > 0) {
        watchdogArmed = true;
        break;
      }
    }
    expect(watchdogArmed).toBe(true);
    expect(mocks.state.stopCalls).toBe(0);

    deactivate();
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }

    expect(mocks.state.stopCalls).toBe(1);
    expect(mocks.state.startCalls).toBe(1);
    expect(vi.getTimerCount()).toBe(0);

    mocks.state.holdStart = false;
    mocks.state.releaseStart = undefined;
    await activate(context);
    expect(mocks.state.startCalls).toBe(2);
    expect(mocks.state.stopCalls).toBe(1);

    deactivate();
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.stopCalls).toBe(2);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops a started client when deactivate is queued from client.start", async () => {
    const context = makeContext();
    mocks.state.holdStart = true;

    const activating = activate(context);
    await waitForHeldStart();
    await activating;

    const originalRelease = mocks.state.releaseStart;
    mocks.state.releaseStart = () => {
      originalRelease?.();
      queueMicrotask(() => {
        queueMicrotask(() => {
          deactivate();
        });
      });
    };
    mocks.state.releaseStart();
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }

    expect(mocks.state.startCalls).toBe(1);
    expect(mocks.state.stopCalls).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops a pending initial client only once when deactivate wins the start race", async () => {
    const context = makeContext();
    mocks.state.holdStart = true;

    const activating = activate(context);
    await waitForHeldStart();
    await activating;
    expect(mocks.state.startCalls).toBe(1);
    expect(mocks.state.stopCalls).toBe(0);

    deactivate();
    mocks.state.releaseStart?.();
    for (let i = 0; i < 40; i += 1) {
      await Promise.resolve();
    }

    expect(mocks.state.stopCalls).toBe(1);
    expect(mocks.state.startCalls).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops an already-started client when the session is replaced via subscriptions", async () => {
    const context = makeContext();
    await activate(context);
    for (let i = 0; i < 40 && mocks.state.startCalls < 1; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.startCalls).toBe(1);
    expect(mocks.state.stopCalls).toBe(0);

    for (const subscription of context.subscriptions.splice(0)) {
      subscription.dispose();
    }
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.stopCalls).toBe(1);

    await activate(context);
    for (let i = 0; i < 40 && mocks.state.startCalls < 2; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.startCalls).toBe(2);
    expect(mocks.state.stopCalls).toBe(1);

    deactivate();
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.stopCalls).toBe(2);
  });

  it("does not let a disposed session's queued plugin refresh overwrite the live editor config", async () => {
    const context = makeContext();
    mocks.state.configurePluginHold = true;

    await activate(context);
    for (let i = 0; i < 10_000 && !mocks.state.releaseConfigurePlugin; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.releaseConfigurePlugin).toBeDefined();
    expect(carrierSourceWrites()).toEqual([["/proj/App.vue"]]);

    const closed = vueDocument;
    (workspace as { textDocuments: unknown[] }).textDocuments.length = 0;
    for (const listener of [...mocks.state.closeListeners]) {
      listener(closed);
    }

    deactivate();

    const replacement = {
      ...vueDocument,
      uri: {
        scheme: "file",
        fsPath: "/proj/New.vue",
        toString: () => "file:///proj/New.vue",
      },
    };
    (workspace as { textDocuments: unknown[] }).textDocuments.push(replacement);
    (window as { activeTextEditor?: { document: unknown } }).activeTextEditor = {
      document: replacement,
    };
    mocks.state.configurePluginHold = false;
    await activate(context);
    for (let i = 0; i < 40; i += 1) {
      await Promise.resolve();
    }

    mocks.state.releaseConfigurePlugin?.();
    for (let i = 0; i < 40; i += 1) {
      await Promise.resolve();
    }

    expect(carrierSourceWrites()).toEqual([["/proj/App.vue"], ["/proj/New.vue"]]);
  });

  it("cancels Ready-triggered Claude notification work with its activation session", async () => {
    const previousClaude = process.env.CLAUDE_CODE;
    process.env.CLAUDE_CODE = "1";
    try {
      const context = makeContext();
      await activate(context);
      for (let i = 0; i < 40 && mocks.state.startCalls < 1; i += 1) {
        await Promise.resolve();
      }
      fireLatestNotification("$/verter/ready", { gen: 1 });
      expect(mocks.state.infoMessages).toEqual([]);

      deactivate();

      await activate(context);
      for (let i = 0; i < 40 && mocks.state.startCalls < 2; i += 1) {
        await Promise.resolve();
      }
      fireLatestNotification("$/verter/ready", { gen: 2 });
      await vi.advanceTimersByTimeAsync(1);

      expect(
        mocks.state.infoMessages.filter((message) => message.includes("Claude Code detected")),
      ).toHaveLength(1);
    } finally {
      if (previousClaude === undefined) {
        delete process.env.CLAUDE_CODE;
      } else {
        process.env.CLAUDE_CODE = previousClaude;
      }
    }
  });
});

/**
 * A lost server — its process exited, its connection kept failing — is
 * recovered by the restart supervisor alone. The language client's own
 * restart-on-close forgets crashes older than three minutes, so behind the
 * supervisor it would restart a slowly leaking server forever.
 */
describe("one crash-recovery authority", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = false;
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("restarts a crashed server through the supervisor once, never through the client", async () => {
    const lifetime = new StartAttemptScope();
    await startAttempt(makeContext(), lifetime);
    const first = errorHandlerOf(0);

    // The client is told not to restart itself, and the crash is shown.
    expect(first.closed()).toEqual({ action: 1, handled: true });
    // The same loss reported again (the failed start a close causes) is one loss.
    first.closed();
    expect(liveStatusBarItem()?.text).toMatch(/restarting \(1\/5\)/);

    await vi.advanceTimersByTimeAsync(DEFAULT_RESTART_POLICY.initialBackoffMs);
    expect(mocks.state.startCalls).toBe(2);
    expect(liveStatusBarItem()?.text).toBe("$(sync~spin) Verter");

    // A late close from the replaced client is stale and recovers nothing.
    first.closed();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(mocks.state.startCalls).toBe(2);

    lifetime.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("ends a crash loop in a visible stopped state that only a manual restart leaves", async () => {
    const lifetime = new StartAttemptScope();
    const server = await startAttempt(makeContext(), lifetime);
    const latest = () => errorHandlerOf(mocks.state.createdClientOptions.length - 1);
    const max = DEFAULT_RESTART_POLICY.maxAutomaticRestarts;

    // Every restarted server dies again well inside the sustained-health window.
    for (let attempt = 1; attempt <= max; attempt += 1) {
      latest().closed();
      await vi.advanceTimersByTimeAsync(
        DEFAULT_RESTART_POLICY.initialBackoffMs * 2 ** (attempt - 1),
      );
      expect(mocks.state.startCalls).toBe(1 + attempt);
    }
    latest().closed();

    expect(liveStatusBarItem()?.text).toMatch(/stopped/);
    expect(liveStatusBarItem()?.command).toBe("verter.restartLanguageServer");
    expect(mocks.errorMessages.some((m) => /Restart Language Server/.test(m))).toBe(true);
    await vi.advanceTimersByTimeAsync(900_000);
    expect(mocks.state.startCalls).toBe(1 + max);
    expect(vi.getTimerCount()).toBe(0);

    await server.restart(true);
    expect(mocks.state.startCalls).toBe(2 + max);
    expect(liveStatusBarItem()?.text).toBe("$(sync~spin) Verter");
    expect(liveStatusBarItem()?.command).toBe("verter.selectTypeProvider");

    lifetime.dispose();
  });

  it("stops a frozen server whose loss is detected after the restart budget is spent", async () => {
    const lifetime = new StartAttemptScope();
    await startAttempt(makeContext(), lifetime);
    const latest = () => errorHandlerOf(mocks.state.createdClientOptions.length - 1);
    const max = DEFAULT_RESTART_POLICY.maxAutomaticRestarts;

    for (let attempt = 1; attempt <= max; attempt += 1) {
      latest().closed();
      await vi.advanceTimersByTimeAsync(
        DEFAULT_RESTART_POLICY.initialBackoffMs * 2 ** (attempt - 1),
      );
    }
    expect(mocks.state.startCalls).toBe(1 + max);
    const stopsBeforeFreeze = mocks.state.stopCalls;

    // The last replacement stays alive but never answers: only the heartbeat
    // watchdog notices, with no `closed` to follow.
    await vi.advanceTimersByTimeAsync(120_000);

    expect(liveStatusBarItem()?.text).toMatch(/stopped/);
    expect(mocks.state.startCalls).toBe(1 + max);
    expect(mocks.state.stopCalls).toBe(stopsBeforeFreeze + 1);

    lifetime.dispose();
  });

  it("shuts down a connection that keeps failing and recovers it", async () => {
    const lifetime = new StartAttemptScope();
    await startAttempt(makeContext(), lifetime);
    const handler = errorHandlerOf(0);

    expect(handler.error(new Error("write EPIPE"), undefined, 3)).toEqual({ action: 1 });
    expect(mocks.state.startCalls).toBe(1);
    expect(handler.error(new Error("write EPIPE"), undefined, 4)).toEqual({
      action: 2,
      handled: true,
    });

    await vi.advanceTimersByTimeAsync(DEFAULT_RESTART_POLICY.initialBackoffMs);
    expect(mocks.state.startCalls).toBe(2);

    lifetime.dispose();
  });

  it("ignores a replaced client's server notifications", async () => {
    const lifetime = new StartAttemptScope();
    const onReady = vi.fn();
    const onCarrierStoreChanged = vi.fn();
    const onCarrierStoreReady = vi.fn();
    const server = await activateVueLanguageServer(makeContext(), log, undefined, {
      lifetime,
      onReady,
      onCarrierStoreChanged,
      onCarrierStoreReady,
    });
    await server.restart(false);
    const startsAfterRestart = mocks.state.startCalls;

    // Queued behind the replacement: none of these describe the serving server.
    fireNotificationOn(0, "$/verter/ready", { gen: 1 });
    fireNotificationOn(0, "$/verter/carrierStoreChanged", {});
    fireNotificationOn(0, "$/verter/carrierStoreReady", { carrierStoreDir: "/stale" });
    expect(onReady).not.toHaveBeenCalled();
    expect(onCarrierStoreChanged).not.toHaveBeenCalled();
    expect(onCarrierStoreReady).not.toHaveBeenCalled();

    // A stale heartbeat must not shorten the replacement's initial grace window.
    fireNotificationOn(0, "$/verter/heartbeat", {});
    await vi.advanceTimersByTimeAsync(45_000);
    expect(mocks.state.startCalls).toBe(startsAfterRestart);

    fireNotificationOn(1, "$/verter/ready", { gen: 1 });
    expect(onReady).toHaveBeenCalledTimes(1);

    lifetime.dispose();
  });

  it("ignores a replaced client's engine pid and trust-prompt notifications", async () => {
    const lifetime = new StartAttemptScope();
    const server = await startAttempt(makeContext(), lifetime);
    const kill = vi.spyOn(process, "kill").mockImplementation(() => true);
    try {
      await server.restart(false);

      // Queued behind the replacement: the retired server's pid may be recycled
      // by the time the current client closes, and its trust prompt is not the
      // current server's to raise.
      fireNotificationOn(0, "$/verter/typeProviderStarted", { pid: 424242, kind: "tsgo" });
      fireNotificationOn(0, "$/verter/tsgoStarted", { pid: 424243 });
      fireNotificationOn(0, "$/verter/viteConfigTrustRequired", {
        configPath: "/stale/vite.config.ts",
        workspaceRoot: "/stale",
        reason: "dynamic",
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(mocks.warningMessages).toEqual([]);

      fireNotificationOn(1, "$/verter/typeProviderStarted", { pid: 515151, kind: "tsgo" });
      errorHandlerOf(1).closed();
      expect(kill.mock.calls.map(([pid]) => pid)).toEqual([515151]);
    } finally {
      kill.mockRestore();
      lifetime.dispose();
    }
  });

  it("recovers a crash of a server whose slow replacement outlived the old watchdog", async () => {
    const lifetime = new StartAttemptScope();
    const server = await startAttempt(makeContext(), lifetime);

    // The replacement takes longer to start than the replaced server's watchdog
    // had left; that watchdog must be gone, not report the replacement as lost.
    await vi.advanceTimersByTimeAsync(20_000);
    mocks.state.holdStart = true;
    const restarting = server.restart(false);
    await vi.advanceTimersByTimeAsync(55_000);
    mocks.state.releaseStart?.();
    mocks.state.holdStart = false;
    await restarting;
    const startsBeforeCrash = mocks.state.startCalls;
    expect(startsBeforeCrash).toBe(2);

    errorHandlerOf(1).closed();
    expect(liveStatusBarItem()?.text).toMatch(/restarting \(1\/5\)/);
    await vi.advanceTimersByTimeAsync(DEFAULT_RESTART_POLICY.initialBackoffMs);
    expect(mocks.state.startCalls).toBe(startsBeforeCrash + 1);

    lifetime.dispose();
  });

  it("never shows a replaced client's engine status as the current one", async () => {
    const lifetime = new StartAttemptScope();
    const server = await startAttempt(makeContext(), lifetime);
    const status = { kind: "tsgo", topology: "managed-tsgo" };

    fireNotificationOn(0, "$/verter/typeProviderStatus", status);
    expect(liveStatusBarItem()?.text).toMatch(/tsgo \(managed\)/);

    await server.restart(false);
    expect(liveStatusBarItem()?.text).toBe("$(sync~spin) Verter");
    fireNotificationOn(0, "$/verter/typeProviderStatus", status);
    expect(liveStatusBarItem()?.text).toBe("$(sync~spin) Verter");

    fireNotificationOn(1, "$/verter/typeProviderStatus", status);
    expect(liveStatusBarItem()?.text).toMatch(/tsgo \(managed\)/);

    lifetime.dispose();
  });
});

/**
 * The extension starts a server for a framework document, not for activation
 * itself or for a setting change, and restarts a running one once per change.
 */
describe("activation on demand", () => {
  const vueDocument = {
    languageId: "vue",
    version: 1,
    getText: () => "",
    uri: {
      scheme: "file",
      fsPath: "/proj/App.vue",
      toString: () => "file:///proj/App.vue",
    },
  };
  const documents = () => workspace.textDocuments as unknown as unknown[];
  const editor = window as unknown as { activeTextEditor?: { document: unknown } };
  const changeSetting = (setting: string) => {
    for (const listener of [...mocks.state.configurationListeners]) {
      listener({ affectsConfiguration: (section: string) => section === setting });
    }
  };

  beforeEach(() => {
    vi.useFakeTimers({
      toFake: ["setTimeout", "setInterval", "clearTimeout", "clearInterval"],
    });
    resetHostRuntimeMocks();
    mocks.state.startShouldFail = false;
    mocks.configurationValues["verter.mcp.enabled"] = false;
    documents().length = 0;
    editor.activeTextEditor = undefined;
  });

  afterEach(() => {
    deactivate();
    documents().length = 0;
    editor.activeTextEditor = undefined;
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("starts no server without a framework document, even when a server setting changes", async () => {
    await activate(makeContext());

    changeSetting("verter.typeProvider");
    await vi.advanceTimersByTimeAsync(1_000);

    expect(mocks.state.createdClients).toHaveLength(0);
    expect(mocks.state.startCalls).toBe(0);
  });

  it("restarts a running server exactly once when the type provider changes", async () => {
    documents().push(vueDocument);
    editor.activeTextEditor = { document: vueDocument };
    await activate(makeContext());
    for (let i = 0; i < 40 && mocks.state.startCalls < 1; i += 1) {
      await Promise.resolve();
    }
    expect(mocks.state.startCalls).toBe(1);

    changeSetting("verter.typeProvider");
    await vi.advanceTimersByTimeAsync(1_000);

    expect(mocks.state.createdClients).toHaveLength(2);
    expect(mocks.state.startCalls).toBe(2);
    expect(mocks.state.stopCalls).toBe(1);
  });
});

function fireNotificationOn(clientIndex: number, type: string, params: unknown) {
  for (const handler of mocks.state.createdClients[clientIndex]?.handlers.get(type) ?? []) {
    handler(params);
  }
}

function fireLatestNotification(type: string, params: unknown) {
  const client = mocks.state.createdClients.at(-1);
  for (const handler of client?.handlers.get(type) ?? []) {
    handler(params);
  }
}

function carrierSourceWrites(): string[][] {
  return mocks.state.configurePluginConfigs.map((config) => {
    const sources = (config as { activeCarrierSources?: unknown }).activeCarrierSources;
    return Array.isArray(sources) ? (sources as string[]) : [];
  });
}
