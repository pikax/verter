/**
 * Route spawner for the corpus gate: the REAL `verter-lsp` binary across the
 * three type-provider routes, modelled on `endurance/spawn.ts` (which itself
 * mirrors `editor-neutral/rawLspDriver.ts`) but with a BOUNDED, BEST-EFFORT
 * startup gate instead of a strict-fatal one.
 *
 * The strict raw-LSP startup gate (matched ready+sync generation, then full
 * quiescence) is correct for hermetic fixtures but demonstrably wedges or
 * times out on a ~731-file real corpus while the server is still usable — and
 * "slow-but-answering" is precisely what this gate exists to measure. So:
 *
 *  - `initialize` failing IS fatal (the server is genuinely broken);
 *  - `$/verter/ready` is awaited up to a hard cap, then the session proceeds;
 *  - post-ready settle is a best-effort capped quiescence poll;
 *  - every startup observation (ready/sync seen, quiesced, timings) is
 *    RECORDED in the route report rather than deciding pass/fail here.
 *
 * The server environment is the editor-neutral one (provider-owned completion
 * evidence), so results are comparable with the editor-neutral contract lane.
 */
import { existsSync, mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";

import {
  GET_STATISTICS_METHOD,
  TYPE_PROVIDER_SYNC_COMPLETE_METHOD,
  VERTER_READY_METHOD,
} from "../core/startupGate.js";
import { extractQuiescenceCounters, pollUntilQuiesced } from "../core/quiescence.js";
import { editorNeutralServerEnvironment } from "../editor-neutral/rawLspDriver.js";
import type { CorpusGateRoute, CorpusRouteStartup } from "./types.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
/** `packages/dx-harness/src/corpus-gate` → repo root (same depth from `dist`). */
export const REPO_ROOT = path.resolve(HERE, "..", "..", "..", "..");

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

/** Mirrors `resolveTsgoBinary` in editor-neutral/rawLspDriver.ts. */
async function resolveTsgoBinary(repoRoot: string, explicit?: string): Promise<string> {
  if (explicit) return requireFile("tsgo binary", explicit);
  if (process.env.VERTER_TSGO_BIN) {
    return requireFile("VERTER_TSGO_BIN", process.env.VERTER_TSGO_BIN);
  }
  // TypeScript 7's package owns the platform-aware executable resolver.
  const resolver = path.join(repoRoot, "node_modules", "typescript", "lib", "getExePath.js");
  requireFile("TypeScript executable resolver", resolver);
  const module = (await import(pathToFileURL(resolver).href)) as { default?: () => string };
  if (typeof module.default !== "function") {
    throw new Error(`TypeScript executable resolver has no default function: ${resolver}`);
  }
  return requireFile("TypeScript 7 native executable", module.default());
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

export interface SpawnCorpusGateLspOptions {
  readonly repoRoot?: string;
  readonly lspBin?: string;
  readonly tsgoBin?: string;
  /** Hard cap on the wait for `$/verter/ready` (ms). */
  readonly readyCapMs: number;
  /** Hard cap on the best-effort post-ready settle (ms). */
  readonly settleCapMs: number;
  /** Per-`getStatistics` request timeout during the settle poll (ms). */
  readonly statisticsTimeoutMs: number;
}

export interface CorpusGateLspHandle {
  readonly client: LspClient;
  /** Present only on the shared-tsgo route. */
  readonly relay?: LspClient;
  readonly route: CorpusGateRoute;
  readonly startup: CorpusRouteStartup;
  /** Latest provider child PID from `$/verter/typeProviderStarted`, if any. */
  providerPid(): number | null;
  /** Graceful shutdown + force-kill fallback; never hangs, idempotent. */
  dispose(): Promise<void>;
}

/**
 * Spawn verter-lsp for `route` against `corpusDir` (read-only), drive it
 * through initialize + the BOUNDED readiness wait, and return the live handle
 * with recorded startup evidence. Throws only when spawn/initialize itself
 * fails; a non-quiesced or ready-capped startup is evidence, not an error.
 */
export async function spawnCorpusGateLsp(
  route: CorpusGateRoute,
  corpusDir: string,
  options: SpawnCorpusGateLspOptions,
): Promise<CorpusGateLspHandle> {
  const repoRoot = path.resolve(options.repoRoot ?? REPO_ROOT);
  const root = path.resolve(corpusDir);
  const lspBin = requireFile(
    "verter-lsp binary",
    options.lspBin ?? process.env.VERTER_LSP_BIN ?? platformBinary(repoRoot, "verter-lsp"),
  );
  const tsgoBin = await resolveTsgoBinary(repoRoot, options.tsgoBin);
  const tsdk = path.join(
    repoRoot,
    "packages",
    "typescript-plugin",
    "node_modules",
    "typescript",
    "lib",
  );
  if (route === "tsserver" && !existsSync(path.join(tsdk, "tsserver.js"))) {
    throw new Error(`tsserver SDK is missing tsserver.js: ${tsdk}`);
  }
  const pluginPath = path.join(repoRoot, "packages", "typescript-plugin", "dist");
  if (route === "tsserver" && !existsSync(path.join(pluginPath, "index.js"))) {
    throw new Error(`TypeScript plugin build is missing index.js: ${pluginPath}`);
  }

  const rootUri = pathToFileURL(root).href;
  let relay: LspClient | undefined;
  let sharedTemp: string | undefined;
  let controlDir: string | undefined;
  let sessionKey: string | undefined;

  const removeSharedTemp = () => {
    if (!sharedTemp) return;
    const resolved = path.resolve(sharedTemp);
    if (
      path.dirname(resolved) === path.resolve(tmpdir()) &&
      path.basename(resolved).startsWith("verter-corpus-shared-")
    ) {
      rmSync(resolved, { recursive: true, force: true });
    }
  };

  if (route === "shared-tsgo") {
    const relayShimBin = requireFile(
      "verter-relay-shim binary",
      process.env.VERTER_RELAY_SHIM_BIN ?? platformBinary(repoRoot, "verter-relay-shim"),
    );
    sharedTemp = mkdtempSync(path.join(tmpdir(), "verter-corpus-shared-"));
    controlDir = path.join(sharedTemp, "control");
    sessionKey = `corpus-${process.pid}-${Date.now()}`;
    relay = new LspClient(
      "corpus-editor-owned-tsgo-relay",
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
      root,
      { defaultTimeout: 45_000 },
    );
    const initialized = (await relay.initialize(
      {
        processId: process.pid,
        rootUri,
        capabilities: {},
        workspaceFolders: [{ uri: rootUri, name: "corpus-gate" }],
      },
      45_000,
    )) as { serverInfo?: { version?: unknown } };
    const observedVersion = initialized.serverInfo?.version;
    if (typeof observedVersion !== "string" || observedVersion.length === 0) {
      throw new Error(`relay did not expose the real editor-owned TypeScript version`);
    }
    relay.sendNotification("initialized", {});
    await waitForRelayAdvertisement(controlDir, 15_000);
  }

  const args = [root, `--type-provider=${route}`];
  if (route === "tsserver") {
    args.push(`--tsdk=${tsdk}`, `--plugin-path=${pluginPath}`);
  }
  if (route === "shared-tsgo") {
    args.push(`--shared-control-dir=${controlDir}`, `--shared-session-key=${sessionKey}`);
  }

  let readyObserved = false;
  let syncObserved = false;
  const providerStarts: number[] = [];
  const client = new LspClient("verter-lsp", lspBin, args, root, {
    defaultTimeout: 30_000,
    env: editorNeutralServerEnvironment(tsgoBin),
    stderr: { maxBytes: 16 * 1024 * 1024 },
    onAnyNotification(method, params) {
      if (method === VERTER_READY_METHOD) readyObserved = true;
      else if (method === TYPE_PROVIDER_SYNC_COMPLETE_METHOD) syncObserved = true;
      else if (method === "$/verter/typeProviderStarted") {
        const pid = (params as { pid?: unknown } | null | undefined)?.pid;
        if (Number.isSafeInteger(pid) && (pid as number) > 0) providerStarts.push(pid as number);
      }
    },
  });

  let disposed = false;
  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    if (client.isAlive()) {
      await client.sendRequest("shutdown", {}, 5_000).catch(() => undefined);
      client.sendNotification("exit", {});
    }
    await client.kill().catch(() => {});
    if (relay?.isAlive()) {
      await relay.sendRequest("shutdown", {}, 5_000).catch(() => undefined);
      relay.sendNotification("exit", {});
    }
    await relay?.kill().catch(() => {});
    removeSharedTemp();
  };

  try {
    const initializeStart = Date.now();
    await client.initialize(
      {
        processId: process.pid,
        rootUri,
        workspaceFolders: [{ uri: rootUri, name: "corpus-gate" }],
        capabilities: {
          general: { positionEncodings: ["utf-16", "utf-8", "utf-32"] },
          workspace: { workspaceFolders: true, workspaceEdit: { documentChanges: true } },
          textDocument: {
            completion: { completionItem: { snippetSupport: true } },
            definition: { linkSupport: true },
            hover: { contentFormat: ["markdown", "plaintext"] },
            publishDiagnostics: { relatedInformation: true },
            rename: { prepareSupport: true },
          },
        },
      },
      60_000,
    );
    const initializeMs = Date.now() - initializeStart;
    client.sendNotification("initialized", {});

    // Bounded readiness: proceed on `$/verter/ready` OR the hard cap. The
    // strict sync+quiescence gate is deliberately NOT required — on a real
    // corpus the provider can churn for minutes and slow per-request results
    // are themselves the finding this gate measures.
    const settleStart = Date.now();
    const readyDeadline = settleStart + options.readyCapMs;
    while (!readyObserved && Date.now() < readyDeadline && client.isAlive()) {
      await new Promise((resolve) => {
        const timer = setTimeout(resolve, 250);
        timer.unref?.();
      });
    }
    if (!client.isAlive()) throw new Error("verter-lsp died during startup readiness wait");

    // Best-effort settle: capped counter quiescence so early probes are not
    // trivially racing the first workspace scan. Failure here is evidence.
    let quiesced = false;
    try {
      const settle = await pollUntilQuiesced(
        async () =>
          extractQuiescenceCounters(
            await client.sendRequest(GET_STATISTICS_METHOD, {}, options.statisticsTimeoutMs),
          ),
        () => [],
        { timeoutMs: options.settleCapMs },
      );
      quiesced = settle.quiesced;
    } catch {
      // A statistics request failing during settle is recorded via quiesced=false;
      // the per-request liveness checks during probing decide wedge-ness.
      quiesced = false;
    }

    const startup: CorpusRouteStartup = {
      initializeMs,
      readyObserved,
      syncObserved,
      quiesced,
      settleMs: Date.now() - settleStart,
    };
    return {
      client,
      relay,
      route,
      startup,
      providerPid: () => providerStarts.at(-1) ?? relay?.process.pid ?? null,
      dispose,
    };
  } catch (error) {
    await dispose();
    throw error;
  }
}
