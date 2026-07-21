/**
 * Self-contained spawner for the REAL `verter-lsp` binary across the three
 * type-provider routes (tsserver | tsgo | shared-tsgo), modelled on
 * `editor-neutral/rawLspDriver.ts` but exposing the raw `LspClient` so
 * endurance scenarios can drive didOpen/didChange traffic the editor-neutral
 * driver does not.
 *
 *  - tsgo:        `verter-lsp <root> --type-provider=tsgo` (+ VERTER_TSGO_BIN)
 *  - tsserver:    `verter-lsp <root> --type-provider=tsserver --tsdk=… --plugin-path=…`
 *  - shared-tsgo: spawn `verter-relay-shim` as a second LspClient, await its
 *                 control-dir advertisement, then start verter-lsp with
 *                 `--type-provider=shared-tsgo --shared-control-dir=… --shared-session-key=…`.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";

import { awaitRawLspStartup } from "../core/startupGate.js";
import type { EnduranceProviderRoute, ProviderRuntimeAttestation } from "./types.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
/** `packages/dx-harness/src/endurance` → repo root (same depth from `dist/endurance`). */
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
  const module = (await import(pathToFileURL(resolver).href)) as {
    default?: () => string;
  };
  if (typeof module.default !== "function") {
    throw new Error(`TypeScript executable resolver has no default function: ${resolver}`);
  }
  return requireFile("TypeScript 7 native executable", module.default());
}

/**
 * The tsserver route needs `@verter/typescript-plugin`'s dist. Build ONLY that
 * package when it is missing (the repo's verter-lsp binary itself is never
 * rebuilt by this harness).
 */
function ensureTypescriptPluginBuilt(repoRoot: string, pluginPath: string): void {
  if (existsSync(path.join(pluginPath, "index.js"))) return;
  execFileSync(
    process.platform === "win32" ? "pnpm.cmd" : "pnpm",
    ["--filter", "@verter/typescript-plugin", "build"],
    { cwd: repoRoot, stdio: "inherit" },
  );
  if (!existsSync(path.join(pluginPath, "index.js"))) {
    throw new Error(`TypeScript plugin build did not produce index.js: ${pluginPath}`);
  }
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

export interface SpawnEnduranceLspOptions {
  readonly repoRoot?: string;
  readonly lspBin?: string;
  readonly tsgoBin?: string;
  /** Total startup-gate budget (matched generation + quiescence). */
  readonly readyTimeoutMs?: number;
  /** Extra environment merged over the default server environment. */
  readonly env?: Readonly<Record<string, string>>;
}

export interface EnduranceLspHandle {
  readonly client: LspClient;
  /** Present only on the shared-tsgo route. */
  readonly relay?: LspClient;
  readonly route: EnduranceProviderRoute;
  readonly workspaceRoot: string;
  /** Snapshot provider-process evidence without consulting the verter-lsp PID. */
  providerAttestation(): ProviderRuntimeAttestation;
  /** Graceful shutdown + force-kill fallback; never hangs, idempotent. */
  dispose(): Promise<void>;
}

interface ProviderStartedNotification {
  readonly pid?: number;
  readonly kind?: string;
}

export function parseProviderRuntimeAttestation(stderr: string): {
  restartLogCount: number;
  reloadProjectsCount: number;
} {
  const restartLogCount = (stderr.match(/restarted successfully \(attempt\s+\d+\)/g) ?? []).length;
  const reloadProjectsCount = (
    stderr.match(
      /\[verter-meta-trace\][^\r\n]*event=start[^\r\n]*name="tsserver_transport_command"[^\r\n]*detail="command=reloadProjects\b/g,
    ) ?? []
  ).length;
  return { restartLogCount, reloadProjectsCount };
}

function processIsAlive(pid: number | null): boolean {
  if (pid === null || !Number.isSafeInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/**
 * Spawn verter-lsp for `route` against `workspaceRoot`, drive it through
 * initialize + the raw startup gate, and return the live client. The caller
 * owns document traffic from here on.
 */
export async function spawnEnduranceLsp(
  route: EnduranceProviderRoute,
  workspaceRoot: string,
  options: SpawnEnduranceLspOptions = {},
): Promise<EnduranceLspHandle> {
  const repoRoot = path.resolve(options.repoRoot ?? REPO_ROOT);
  const root = path.resolve(workspaceRoot);
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
  if (route === "tsserver") ensureTypescriptPluginBuilt(repoRoot, pluginPath);

  const env: Record<string, string> = {
    VERTER_TSGO_BIN: tsgoBin,
    VERTER_LOG: "info",
    VERTER_E2E_TEST: "1",
    // Emits a stable start-event for every tsserver command. Receipts count
    // reloadProjects from that emitted evidence, never from an assumed zero.
    VERTER_TYPE_RUNTIME_TRACE: "1",
    // NOTE: VERTER_E2E_PROVIDER_ONLY_COMPLETIONS is deliberately NOT set — that
    // flag zeroes Verter's NATIVE completion producer (see server/mod.rs
    // `provider_only_completions`), and the D1 contract this harness asserts
    // (template component-prop-name completions) IS the native producer.
    ...options.env,
  };
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
      path.basename(resolved).startsWith("verter-endurance-shared-")
    ) {
      rmSync(resolved, { recursive: true, force: true });
    }
  };

  if (route === "shared-tsgo") {
    const relayShimBin = requireFile(
      "verter-relay-shim binary",
      process.env.VERTER_RELAY_SHIM_BIN ?? platformBinary(repoRoot, "verter-relay-shim"),
    );
    sharedTemp = mkdtempSync(path.join(tmpdir(), "verter-endurance-shared-"));
    controlDir = path.join(sharedTemp, "control");
    sessionKey = `endurance-${process.pid}-${Date.now()}`;
    relay = new LspClient(
      "endurance-editor-owned-tsgo-relay",
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
        workspaceFolders: [{ uri: rootUri, name: "endurance" }],
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

  const providerStarts: ProviderStartedNotification[] = [];
  const client = new LspClient("verter-lsp", lspBin, args, root, {
    defaultTimeout: 30_000,
    env,
    onAnyNotification(method, params) {
      if (method === "$/verter/typeProviderStarted") providerStarts.push(params ?? {});
    },
  });

  const providerAttestation = (): ProviderRuntimeAttestation => {
    const runtime = parseProviderRuntimeAttestation(client.stderr.text());
    if (route === "shared-tsgo") {
      const pid = relay?.process.pid ?? null;
      return {
        pid,
        kind: "shared-tsgo-relay",
        evidence: "editor-owned-relay",
        aliveAtEnd: relay?.isAlive() === true && processIsAlive(pid),
        restartCount: runtime.restartLogCount,
        providerStartCount: pid === null ? 0 : 1,
        reloadProjectsCount: runtime.reloadProjectsCount,
        restartLogCount: runtime.restartLogCount,
      };
    }
    const validStarts = providerStarts.filter(
      (item): item is { pid: number; kind?: string } =>
        Number.isSafeInteger(item.pid) && (item.pid ?? 0) > 0,
    );
    const latest = validStarts.at(-1);
    const pid = latest?.pid ?? null;
    return {
      pid,
      kind: latest?.kind ?? route,
      evidence: "typeProviderStarted",
      aliveAtEnd: processIsAlive(pid),
      restartCount: Math.max(Math.max(0, validStarts.length - 1), runtime.restartLogCount),
      providerStartCount: validStarts.length,
      reloadProjectsCount: runtime.reloadProjectsCount,
      restartLogCount: runtime.restartLogCount,
    };
  };

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
    await client.initialize(
      {
        processId: process.pid,
        rootUri,
        workspaceFolders: [{ uri: rootUri, name: "endurance" }],
        capabilities: {
          workspace: {
            workspaceFolders: true,
            workspaceEdit: { documentChanges: true },
          },
          textDocument: {
            completion: { completionItem: { snippetSupport: true } },
            definition: { linkSupport: true },
            hover: { contentFormat: ["markdown", "plaintext"] },
            publishDiagnostics: { relatedInformation: true },
          },
        },
      },
      30_000,
    );
    const startup = awaitRawLspStartup(client, {
      readyTimeoutMs: options.readyTimeoutMs ?? 120_000,
      statisticsTimeoutMs: 10_000,
      quiescence: { timeoutMs: 30_000 },
    });
    client.sendNotification("initialized", {});
    await startup;
    return { client, relay, route, workspaceRoot: root, providerAttestation, dispose };
  } catch (error) {
    await dispose();
    throw error;
  }
}
