import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * A failed `client.start()` must leave nothing behind.
 *
 * Everything one language-server start attempt creates — event subscriptions,
 * the type-provider status bar item, and the heartbeat watchdog timer — belongs
 * to that attempt, not to the extension. Before the per-attempt disposal scope
 * existed, each failed attempt leaked its whole subscription set, another status
 * bar item, and an UNCANCELLABLE 60s restart timer that kept firing against a
 * dead client, so an LSP that cannot launch produced an ever-growing row of
 * status bar items and a restart storm.
 *
 * The assertions are on DISPOSAL (`context.subscriptions`, `item.dispose()`
 * calls, armed timer count), never on rendered UI text.
 */

const mocks = vi.hoisted(() => {
  const statusBarItems: { disposed: boolean }[] = [];
  const diagnosticCollections: { disposed: boolean }[] = [];
  const state = { startShouldFail: true };
  const configuration = {
    get: (_key: string, fallback?: unknown) => fallback,
    inspect: () => undefined,
    update: async () => {},
  };
  return { statusBarItems, diagnosticCollections, state, configuration };
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
      showInformationMessage: async () => undefined,
      showWarningMessage: async () => undefined,
      showErrorMessage: async () => undefined,
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
      getConfiguration: () => mocks.configuration,
      onDidOpenTextDocument: subscribe,
      onDidChangeTextDocument: subscribe,
      onDidCloseTextDocument: subscribe,
      onDidSaveTextDocument: subscribe,
      onDidChangeConfiguration: subscribe,
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
      executeCommand: async () => undefined,
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
    extensions: { getExtension: () => undefined, all: [] as unknown[] },
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
  };
});

vi.mock("vscode-languageclient/node", () => ({
  LanguageClient: class {
    protocol2CodeConverter = {};
    onNotification() {
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
      if (mocks.state.startShouldFail) {
        throw new Error("verter-lsp exited before the initialize handshake");
      }
    }
    async stop() {}
  },
  State: { Stopped: 1, Running: 2, Starting: 3 },
  TransportKind: { stdio: 0, ipc: 1, pipe: 2, socket: 3 },
  RevealOutputChannelOn: { Info: 1, Warn: 2, Error: 3, Never: 4 },
}));

import { activateVueLanguageServer } from "./extension";

type ActivateParams = Parameters<typeof activateVueLanguageServer>;

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
  } as unknown as ActivateParams[0] & { subscriptions: { dispose(): unknown }[] };
}

const liveStatusBarItems = () => mocks.statusBarItems.filter((item) => !item.disposed).length;

describe("language server start attempt lifetime", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.statusBarItems.length = 0;
    mocks.diagnosticCollections.length = 0;
    mocks.state.startShouldFail = true;
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("releases every subscription, status bar item and armed timer when a start attempt fails", async () => {
    const context = makeContext();

    const readings: { subscriptions: number; statusBarItems: number; timers: number }[] = [];
    for (let attempt = 0; attempt < 4; attempt += 1) {
      await expect(activateVueLanguageServer(context, log)).rejects.toThrow(
        /exited before the initialize handshake/,
      );
      readings.push({
        subscriptions: context.subscriptions.length,
        statusBarItems: liveStatusBarItems(),
        timers: vi.getTimerCount(),
      });
    }

    // A failed attempt owns nothing afterwards — not "fewer than before", zero.
    for (const reading of readings) {
      expect(reading).toEqual({ subscriptions: 0, statusBarItems: 0, timers: 0 });
    }
    // The status bar items were genuinely created and genuinely disposed, so a
    // mock that never created one cannot make this test pass by accident.
    expect(mocks.statusBarItems.length).toBe(4);
    expect(mocks.statusBarItems.every((item) => item.disposed)).toBe(true);
    expect(mocks.diagnosticCollections.every((collection) => collection.disposed)).toBe(true);
  });

  it("cancels the heartbeat watchdog when the extension disposes a started server", async () => {
    mocks.state.startShouldFail = false;
    const context = makeContext();

    await activateVueLanguageServer(context, log);
    // A live attempt is armed: the watchdog timer and the status bar item exist.
    expect(vi.getTimerCount()).toBeGreaterThan(0);
    expect(liveStatusBarItems()).toBe(1);

    for (const subscription of context.subscriptions.splice(0)) {
      subscription.dispose();
    }

    expect(vi.getTimerCount()).toBe(0);
    expect(liveStatusBarItems()).toBe(0);
  });
});
