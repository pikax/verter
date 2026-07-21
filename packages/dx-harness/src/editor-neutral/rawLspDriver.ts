/**
 * Raw-stdio implementation of the shared editor-neutral LSP contract driver.
 *
 * The driver launches the real Verter server and, for `shared-tsgo`, a real
 * relay-shim + real editor-owned TypeScript process. No editor API participates
 * in standard-LSP assertions, which keeps the contract reusable by every client.
 */
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  LspClient,
  type EditorNeutralContractDriver,
  type EditorNeutralProviderRoute,
  type LspDiagnostic,
  type LspPosition,
  type LspWorkspaceEdit,
  type ProviderAttestation,
  type ProviderTopologyAttestation,
} from "@verter/lsp-test-client";

import { extractQuiescenceCounters, pollUntilQuiesced } from "../core/quiescence.js";
import { awaitRawLspStartup, GET_STATISTICS_METHOD } from "../core/startupGate.js";

const DIAGNOSTICS_METHOD = "textDocument/publishDiagnostics";
const TYPE_PROVIDER_STATUS_METHOD = "$/verter/typeProviderStatus";
const TYPE_PROVIDER_STARTED_METHOD = "$/verter/typeProviderStarted";
const DEFAULT_TIMEOUT_MS = 30_000;

/** Environment that keeps editor-neutral completion evidence provider-owned. */
export function editorNeutralServerEnvironment(tsgoBin: string): Readonly<Record<string, string>> {
  return {
    VERTER_TSGO_BIN: tsgoBin,
    VERTER_LOG: "info",
    VERTER_E2E_TEST: "1",
    VERTER_E2E_PROVIDER_ONLY_COMPLETIONS: "1",
  };
}

export interface RawEditorNeutralLspDriverOptions {
  readonly route: EditorNeutralProviderRoute;
  readonly repoRoot: string;
  readonly workspaceRoot: string;
  readonly lspBin?: string;
  readonly relayShimBin?: string;
  readonly tsgoBin?: string;
  readonly tsdk?: string;
  readonly pluginPath?: string;
}

interface StatusNotification {
  readonly kind?: string;
  readonly reason?: string;
  readonly recommendation?: {
    readonly preferred: string;
    readonly reason: string;
    readonly knownGaps: readonly string[];
  };
}

interface StartedNotification {
  readonly pid?: number;
  readonly kind?: string;
}

interface DiagnosticActivity {
  revision: number;
  lastAt: number;
  readonly publishedUris: Set<string>;
}

interface OpenDocument {
  readonly relativePath: string;
  readonly uri: string;
  readonly languageId: string;
  readonly text: string;
}

function requireFile(label: string, candidate: string): string {
  const resolved = path.resolve(candidate);
  if (!existsSync(resolved) || !statSync(resolved).isFile()) {
    throw new Error(`${label} is required and must be a file: ${resolved}`);
  }
  return resolved;
}

function platformBinary(root: string, stem: string): string {
  return path.join(root, "target", "debug", process.platform === "win32" ? `${stem}.exe` : stem);
}

async function resolveTsgoBinary(repoRoot: string, explicit?: string): Promise<string> {
  if (explicit) return requireFile("tsgo binary", explicit);
  if (process.env.VERTER_TSGO_BIN) {
    return requireFile("VERTER_TSGO_BIN", process.env.VERTER_TSGO_BIN);
  }

  // TypeScript 7's package owns the platform-aware executable resolver. Import
  // that resolver rather than mirroring package/platform names in this harness.
  const resolver = path.join(repoRoot, "node_modules", "typescript", "lib", "getExePath.js");
  requireFile("TypeScript executable resolver", resolver);
  const module = (await import(pathToFileURL(resolver).href)) as {
    default?: () => string;
  };
  if (typeof module.default !== "function") {
    throw new Error(`TypeScript executable resolver has no default function: ${resolver}`);
  }
  return requireFile("TypeScript 7 native executable", module.default());
}

function collectWorkspaceDocuments(workspaceRoot: string): readonly OpenDocument[] {
  const srcRoot = path.join(workspaceRoot, "src");
  if (!existsSync(srcRoot) || !statSync(srcRoot).isDirectory()) {
    throw new Error(`editor-neutral fixture has no src directory: ${srcRoot}`);
  }
  const files: string[] = [];
  const visit = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const absolute = path.join(dir, entry.name);
      if (entry.isDirectory()) visit(absolute);
      else if (entry.isFile() && /\.(?:vue|svelte|tsx?|jsx?)$/.test(entry.name))
        files.push(absolute);
    }
  };
  visit(srcRoot);
  files.sort((left, right) => left.localeCompare(right));
  if (files.length === 0) throw new Error(`editor-neutral fixture discovered zero source files`);
  return files.map((absolute) => {
    const relativePath = path.relative(workspaceRoot, absolute).replaceAll("\\", "/");
    const extension = path.extname(absolute);
    const languageId =
      extension === ".vue"
        ? "vue"
        : extension === ".svelte"
          ? "svelte"
          : extension === ".js"
            ? "javascript"
            : extension === ".jsx"
              ? "javascriptreact"
              : extension === ".tsx"
                ? "typescriptreact"
                : "typescript";
    return {
      relativePath,
      uri: pathToFileURL(absolute).href,
      languageId,
      text: readFileSync(absolute, "utf8"),
    };
  });
}

function waitForRelayAdvertisement(controlDir: string, timeoutMs: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = () => {
      let entries: string[] = [];
      try {
        entries = readdirSync(controlDir);
      } catch {
        // The shim creates the directory after its process starts.
      }
      const advertisement = entries.find((entry) => /^verter-relay-shim-.*\.json$/.test(entry));
      if (advertisement) {
        if (timer) clearTimeout(timer);
        resolve(path.join(controlDir, advertisement));
        return;
      }
      if (Date.now() >= deadline) {
        reject(new Error(`real relay did not advertise in ${controlDir} within ${timeoutMs}ms`));
        return;
      }
      timer = setTimeout(poll, 25);
      timer.unref?.();
    };
    poll();
  });
}

function waitForDiagnosticActivitySettled(
  activity: DiagnosticActivity,
  expectedUris: ReadonlySet<string>,
  stableMs: number,
  timeoutMs: number,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = () => {
      const missingUris = [...expectedUris].filter((uri) => !activity.publishedUris.has(uri));
      if (missingUris.length === 0 && Date.now() - activity.lastAt >= stableMs) {
        if (timer) clearTimeout(timer);
        resolve();
        return;
      }
      if (Date.now() >= deadline) {
        reject(
          new Error(
            `publishDiagnostics did not settle within ${timeoutMs}ms ` +
              `(revision=${activity.revision}, missing=${JSON.stringify(missingUris)})`,
          ),
        );
        return;
      }
      timer = setTimeout(poll, 25);
      timer.unref?.();
    };
    poll();
  });
}

/** Real raw-LSP contract driver. Construct with {@link create}; dispose after use. */
export class RawEditorNeutralLspDriver implements EditorNeutralContractDriver {
  readonly route: EditorNeutralProviderRoute;
  readonly sources: ReadonlyMap<string, string>;

  private readonly client: LspClient;
  private readonly documents: ReadonlyMap<string, OpenDocument>;
  private readonly diagnosticsByUri: Map<string, readonly LspDiagnostic[]>;
  private readonly statuses: StatusNotification[];
  private readonly started: StartedNotification[];
  private readonly relay?: LspClient;
  private readonly sharedTemp?: string;
  private readonly relayAdvertisement?: string;

  private constructor(input: {
    route: EditorNeutralProviderRoute;
    client: LspClient;
    documents: readonly OpenDocument[];
    diagnosticsByUri: Map<string, readonly LspDiagnostic[]>;
    statuses: StatusNotification[];
    started: StartedNotification[];
    relay?: LspClient;
    sharedTemp?: string;
    relayAdvertisement?: string;
  }) {
    this.route = input.route;
    this.client = input.client;
    this.diagnosticsByUri = input.diagnosticsByUri;
    this.statuses = input.statuses;
    this.started = input.started;
    this.relay = input.relay;
    this.sharedTemp = input.sharedTemp;
    this.relayAdvertisement = input.relayAdvertisement;
    this.documents = new Map(input.documents.map((document) => [document.relativePath, document]));
    this.sources = new Map(
      input.documents.map((document) => [document.relativePath, document.text]),
    );
  }

  get positionEncoding() {
    return this.client.positionEncoding;
  }

  static async create(
    options: RawEditorNeutralLspDriverOptions,
  ): Promise<RawEditorNeutralLspDriver> {
    const repoRoot = path.resolve(options.repoRoot);
    const workspaceRoot = path.resolve(options.workspaceRoot);
    const lspBin = requireFile(
      "verter-lsp binary",
      options.lspBin ?? process.env.VERTER_LSP_BIN ?? platformBinary(repoRoot, "verter-lsp"),
    );
    const tsgoBin = await resolveTsgoBinary(repoRoot, options.tsgoBin);
    const tsdk = path.resolve(
      options.tsdk ??
        path.join(repoRoot, "packages", "typescript-plugin", "node_modules", "typescript", "lib"),
    );
    if (!existsSync(path.join(tsdk, "tsserver.js"))) {
      throw new Error(`tsserver SDK is missing tsserver.js: ${tsdk}`);
    }
    const pluginPath = path.resolve(
      options.pluginPath ?? path.join(repoRoot, "packages", "typescript-plugin", "dist"),
    );
    if (!existsSync(path.join(pluginPath, "index.js"))) {
      throw new Error(`TypeScript plugin build is missing index.js: ${pluginPath}`);
    }

    const rootUri = pathToFileURL(workspaceRoot).href;
    let relay: LspClient | undefined;
    let sharedTemp: string | undefined;
    let relayAdvertisement: string | undefined;
    let controlDir: string | undefined;
    let sessionKey: string | undefined;

    if (options.route === "shared-tsgo") {
      const relayShimBin = requireFile(
        "verter-relay-shim binary",
        options.relayShimBin ??
          process.env.VERTER_RELAY_SHIM_BIN ??
          platformBinary(repoRoot, "verter-relay-shim"),
      );
      sharedTemp = mkdtempSync(path.join(tmpdir(), "verter-neutral-shared-"));
      controlDir = path.join(sharedTemp, "control");
      sessionKey = `neutral-${process.pid}-${Date.now()}`;
      relay = new LspClient(
        "editor-owned-tsgo-relay",
        relayShimBin,
        [
          "--real-tsgo",
          tsgoBin,
          "--control-dir",
          controlDir,
          "--session-key",
          sessionKey,
          "--",
          "--lsp",
          "--stdio",
        ],
        workspaceRoot,
        { defaultTimeout: 45_000 },
      );
      const initialized = (await relay.initialize(
        {
          processId: process.pid,
          rootUri,
          capabilities: {},
          workspaceFolders: [{ uri: rootUri, name: "editor-neutral-contract" }],
        },
        45_000,
      )) as { serverInfo?: { version?: unknown } };
      const observedVersion = initialized.serverInfo?.version;
      if (typeof observedVersion !== "string" || observedVersion.length === 0) {
        throw new Error(`relay did not expose the real editor-owned TypeScript version`);
      }
      relay.sendNotification("initialized", {});
      relayAdvertisement = await waitForRelayAdvertisement(controlDir, 15_000);
    }

    const diagnosticsByUri = new Map<string, readonly LspDiagnostic[]>();
    const diagnosticActivity: DiagnosticActivity = {
      revision: 0,
      lastAt: 0,
      publishedUris: new Set(),
    };
    const statuses: StatusNotification[] = [];
    const started: StartedNotification[] = [];
    const args = [workspaceRoot, `--type-provider=${options.route}`];
    if (options.route === "tsserver") {
      args.push(`--tsdk=${tsdk}`, `--plugin-path=${pluginPath}`);
    }
    if (options.route === "shared-tsgo") {
      args.push(`--shared-control-dir=${controlDir}`, `--shared-session-key=${sessionKey}`);
    }

    const client = new LspClient("verter-lsp", lspBin, args, workspaceRoot, {
      defaultTimeout: DEFAULT_TIMEOUT_MS,
      env: editorNeutralServerEnvironment(tsgoBin),
      onAnyNotification(method, params) {
        if (method === DIAGNOSTICS_METHOD && typeof params?.uri === "string") {
          diagnosticsByUri.set(
            params.uri,
            Array.isArray(params.diagnostics) ? params.diagnostics : [],
          );
          diagnosticActivity.revision += 1;
          diagnosticActivity.lastAt = Date.now();
          diagnosticActivity.publishedUris.add(params.uri);
        } else if (method === TYPE_PROVIDER_STATUS_METHOD) {
          statuses.push(params ?? {});
        } else if (method === TYPE_PROVIDER_STARTED_METHOD) {
          started.push(params ?? {});
        }
      },
    });

    try {
      await client.initialize(
        {
          processId: process.pid,
          rootUri,
          workspaceFolders: [{ uri: rootUri, name: "editor-neutral-contract" }],
          capabilities: {
            general: { positionEncodings: ["utf-16", "utf-8", "utf-32"] },
            workspace: {
              workspaceFolders: true,
              workspaceEdit: { documentChanges: true },
            },
            textDocument: {
              completion: { completionItem: { snippetSupport: true } },
              definition: { linkSupport: true },
              hover: { contentFormat: ["markdown", "plaintext"] },
              publishDiagnostics: { relatedInformation: true },
              rename: { prepareSupport: true },
            },
          },
        },
        30_000,
      );
      const startup = awaitRawLspStartup(client, {
        readyTimeoutMs: 120_000,
        statisticsTimeoutMs: 10_000,
        quiescence: { timeoutMs: 45_000 },
      });
      client.sendNotification("initialized", {});
      await startup;

      const documents = collectWorkspaceDocuments(workspaceRoot);
      const driver = new RawEditorNeutralLspDriver({
        route: options.route,
        client,
        documents,
        diagnosticsByUri,
        statuses,
        started,
        relay,
        sharedTemp,
        relayAdvertisement,
      });
      for (const document of documents) {
        client.sendNotification("textDocument/didOpen", {
          textDocument: {
            uri: document.uri,
            languageId: document.languageId,
            version: 1,
            text: document.text,
          },
        });
      }
      const settled = await pollUntilQuiesced(
        async () =>
          extractQuiescenceCounters(await client.sendRequest(GET_STATISTICS_METHOD, {}, 10_000)),
        () => [],
        { timeoutMs: 45_000 },
      );
      if (!settled.quiesced) {
        throw new Error(`raw LSP did not quiesce after opening contract documents`);
      }
      // Diagnostics are push-delivered independently of the statistics counters.
      // Require a stable activity window before an empty array can be observed;
      // otherwise an initial empty publication can race the provider's later
      // semantic result and make a deliberate-error control vacuous.
      await waitForDiagnosticActivitySettled(
        diagnosticActivity,
        new Set(documents.map((document) => document.uri)),
        600,
        DEFAULT_TIMEOUT_MS,
      );
      return driver;
    } catch (error) {
      await client.kill().catch(() => {});
      await relay?.kill().catch(() => {});
      if (sharedTemp) RawEditorNeutralLspDriver.removeOwnedSharedTemp(sharedTemp);
      throw error;
    }
  }

  private static removeOwnedSharedTemp(candidate: string): void {
    const resolved = path.resolve(candidate);
    const parent = path.resolve(tmpdir());
    if (
      path.dirname(resolved) !== parent ||
      !path.basename(resolved).startsWith("verter-neutral-shared-")
    ) {
      throw new Error(`refusing to remove non-owned shared temp path: ${resolved}`);
    }
    rmSync(resolved, { recursive: true, force: true });
  }

  private document(relativePath: string): OpenDocument {
    const normalized = relativePath.replaceAll("\\", "/");
    const document = this.documents.get(normalized);
    if (!document) throw new Error(`contract driver did not open ${normalized}`);
    return document;
  }

  private async waitForDiagnostics(uri: string): Promise<readonly LspDiagnostic[]> {
    const current = this.diagnosticsByUri.get(uri);
    if (current !== undefined) return current;
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const finish = (diagnostics: readonly LspDiagnostic[]) => {
        if (timer) clearTimeout(timer);
        this.client.offNotification(DIAGNOSTICS_METHOD, handler);
        resolve(diagnostics);
      };
      const handler = (params: any) => {
        if (params?.uri !== uri) return;
        finish(Array.isArray(params.diagnostics) ? params.diagnostics : []);
      };
      this.client.onNotification(DIAGNOSTICS_METHOD, handler);
      // Close the check/register race against the wildcard capture.
      const captured = this.diagnosticsByUri.get(uri);
      if (captured !== undefined) {
        finish(captured);
        return;
      }
      timer = setTimeout(() => {
        this.client.offNotification(DIAGNOSTICS_METHOD, handler);
        reject(new Error(`no publishDiagnostics notification for ${uri}`));
      }, DEFAULT_TIMEOUT_MS);
      timer.unref();
    });
  }

  async diagnostics(relativePath: string): Promise<readonly LspDiagnostic[]> {
    return this.waitForDiagnostics(this.document(relativePath).uri);
  }

  async hover(relativePath: string, position: LspPosition): Promise<unknown> {
    const document = this.document(relativePath);
    return this.client.sendRequest("textDocument/hover", {
      textDocument: { uri: document.uri },
      position,
    });
  }

  async definition(relativePath: string, position: LspPosition): Promise<unknown> {
    const document = this.document(relativePath);
    return this.client.sendRequest("textDocument/definition", {
      textDocument: { uri: document.uri },
      position,
    });
  }

  async completion(relativePath: string, position: LspPosition): Promise<unknown> {
    const document = this.document(relativePath);
    return this.client.sendRequest("textDocument/completion", {
      textDocument: { uri: document.uri },
      position,
      context: { triggerKind: 2, triggerCharacter: "." },
    });
  }

  async rename(
    relativePath: string,
    position: LspPosition,
    newName: string,
  ): Promise<LspWorkspaceEdit | null> {
    const document = this.document(relativePath);
    return this.client.sendRequest("textDocument/rename", {
      textDocument: { uri: document.uri },
      position,
      newName,
    });
  }

  async attestProvider(): Promise<ProviderAttestation> {
    const status = this.statuses.at(-1);
    if (!status) throw new Error(`no ${TYPE_PROVIDER_STATUS_METHOD} notification was observed`);
    const publicKind = status.kind;
    if (!["tsserver", "tsgo", "editor-tsserver", "none"].includes(publicKind ?? "")) {
      throw new Error(`invalid public provider kind: ${JSON.stringify(publicKind)}`);
    }
    return {
      route: this.route,
      publicKind: publicKind as ProviderAttestation["publicKind"],
      reason: status.reason,
      recommendation: status.recommendation,
      startedKinds: this.started
        .map((notification) => notification.kind)
        .filter((kind): kind is string => typeof kind === "string"),
    };
  }

  async attestTopology(): Promise<ProviderTopologyAttestation> {
    const startedKinds = this.started
      .map((notification) => notification.kind)
      .filter((kind): kind is string => typeof kind === "string");
    const sharedRelayAlive =
      this.route === "shared-tsgo" &&
      this.relay?.isAlive() === true &&
      !!this.relayAdvertisement &&
      existsSync(this.relayAdvertisement);
    return {
      managedFallbackStarted: startedKinds.includes("tsgo"),
      sharedRelayAlive,
      detail: `startedKinds=${JSON.stringify(startedKinds)}, advertisement=${this.relayAdvertisement ?? "none"}`,
    };
  }

  async dispose(): Promise<void> {
    for (const document of this.documents.values()) {
      this.client.sendNotification("textDocument/didClose", {
        textDocument: { uri: document.uri },
      });
    }
    if (this.client.isAlive()) {
      await this.client.sendRequest("shutdown", {}, 5000).catch(() => undefined);
      this.client.sendNotification("exit", {});
    }
    await this.client.kill().catch(() => {});
    if (this.relay?.isAlive()) {
      await this.relay.sendRequest("shutdown", {}, 5000).catch(() => undefined);
      this.relay.sendNotification("exit", {});
    }
    await this.relay?.kill().catch(() => {});
    if (this.sharedTemp) RawEditorNeutralLspDriver.removeOwnedSharedTemp(this.sharedTemp);
  }
}
