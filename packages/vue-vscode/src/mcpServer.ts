/**
 * Standalone verter-mcp server lifecycle for the extension.
 *
 * Since the LSP/MCP decoupling the LSP binary no longer embeds an MCP server:
 * the editor client owns spawning the standalone `verter-mcp` binary (HTTP
 * transport, OS-assigned port by default) and learning its bound port from
 * the one-line JSON readiness record the server prints on stdout — see
 * `crates/verter_mcp/src/readiness.rs` for the encoding contract. Human
 * tracing goes to the child's stderr and is never accepted as port identity.
 *
 * Binary discovery reuses the shared `verter-mcp` launcher package (platform
 * package → workspace `target/{debug,release}` dev build → PATH); the one
 * candidate the launcher cannot know about — the binary staged into the
 * extension's own `bin/` for VSIX packaging — slots in ahead of the PATH
 * fallback here.
 *
 * This module is deliberately `vscode`-free so its logic runs under plain
 * vitest; the extension wires it up in `extension.ts`.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { posix, win32 } from "node:path";
import { createInterface } from "node:readline";

import { serverBinaryCandidates } from "verter-mcp";

/** The stdout record's single top-level key (pinned against the Rust encoder). */
export const MCP_HTTP_READY_RECORD_KEY = "verterMcpHttpReady";

/** The payload of an HTTP readiness record. */
export interface McpHttpReadyRecord {
  readonly port: number;
  readonly url: string;
}

/**
 * Parse one child stdout line as a readiness record.
 *
 * Returns `undefined` for anything that is not a well-formed record naming a
 * real bound port — noise lines, other JSON, a record claiming port 0 or an
 * out-of-range value.
 */
export function parseMcpHttpReadyRecord(line: string): McpHttpReadyRecord | undefined {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return undefined;
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return undefined;
  }
  if (typeof parsed !== "object" || parsed === null) return undefined;
  const record = (parsed as Record<string, unknown>)[MCP_HTTP_READY_RECORD_KEY];
  if (typeof record !== "object" || record === null) return undefined;
  const { port, url } = record as Record<string, unknown>;
  if (typeof port !== "number" || !Number.isInteger(port) || port <= 0 || port > 65535) {
    return undefined;
  }
  if (typeof url !== "string" || url.length === 0) return undefined;
  return { port, url };
}

/** Everything the standalone server argv is derived from. */
export interface McpLaunchConfig {
  /** `verter.mcp.port` — 0 requests an OS-assigned port. */
  readonly port: number;
  /** `verter.mcp.lintPreset`. */
  readonly lintPreset: string;
  /** The workspace root handed to `--project-root`; omitted when absent. */
  readonly rootPath?: string;
  /**
   * The host (extension-host) pid handed to `--client-pid`, binding the
   * server's lifetime to the host the same way `verter-lsp` is bound: the
   * server exits when that process dies, so a hard host kill cannot orphan
   * an HTTP listener. Omitted for standalone agent launches, which own
   * their lifetime through their transport.
   */
  readonly clientPid?: number;
}

/** Assemble the standalone `verter-mcp` argv (never the LSP's retired `--mcp-port`). */
export function buildMcpServerArgs(config: McpLaunchConfig): string[] {
  const args = [
    "--transport",
    "http",
    "--port",
    String(config.port),
    "--lint-preset",
    config.lintPreset,
  ];
  if (config.rootPath) {
    args.push("--project-root", config.rootPath);
  }
  if (config.clientPid !== undefined) {
    args.push("--client-pid", String(config.clientPid));
  }
  return args;
}

/** One place the server binary might live, tagged with its provenance. */
export interface McpBinaryCandidate {
  readonly path: string;
  readonly source: string;
}

/** The extension's own staged VSIX binary (`<extensionPath>/bin/verter-mcp`). */
export function bundledMcpBinaryCandidate(
  extensionPath: string,
  platform: NodeJS.Platform = process.platform,
): McpBinaryCandidate {
  const binaryName = platform === "win32" ? "verter-mcp.exe" : "verter-mcp";
  const targetPath = platform === "win32" ? win32 : posix;
  return { path: targetPath.join(extensionPath, "bin", binaryName), source: "bundled" };
}

/**
 * The launcher's candidates with the bundled VSIX binary slotted in ahead of
 * the bare-name PATH fallback (a local dev build still wins for contributors).
 */
export function orderedMcpBinaryCandidates(
  launcherCandidates: readonly McpBinaryCandidate[],
  bundled: McpBinaryCandidate,
): McpBinaryCandidate[] {
  const ordered = [...launcherCandidates];
  const pathIndex = ordered.findIndex((candidate) => candidate.source === "path");
  ordered.splice(pathIndex === -1 ? ordered.length : pathIndex, 0, bundled);
  return ordered;
}

/**
 * Resolve the standalone server binary for this host, or `undefined` when no
 * candidate exists on disk and no PATH fallback applies.
 */
export function resolveMcpServerBinary(extensionPath: string): McpBinaryCandidate | undefined {
  let launcherCandidates: readonly McpBinaryCandidate[];
  try {
    launcherCandidates = serverBinaryCandidates();
  } catch {
    // The launcher throws on a host no platform package covers; the bundled
    // binary (exact-platform VSIX) may still serve it.
    launcherCandidates = [];
  }
  const candidates = orderedMcpBinaryCandidates(
    launcherCandidates,
    bundledMcpBinaryCandidate(extensionPath),
  );
  for (const candidate of candidates) {
    // The bare PATH name cannot be stat'ed — hand it to spawn, which reports
    // ENOENT through the handle's `ready` rejection if nothing serves it.
    if (candidate.source === "path") return candidate;
    if (existsSync(candidate.path)) return candidate;
  }
  return undefined;
}

/** The logging subset this module needs (satisfied by `LogOutputChannel`). */
export interface McpServerLog {
  info(message: string): void;
  warn(message: string): void;
  debug(message: string): void;
}

/** A spawned standalone server attempt. */
export interface McpServerProcessHandle {
  /** Resolves with the child's readiness record; rejects on exit/spawn-failure/timeout. */
  readonly ready: Promise<McpHttpReadyRecord>;
  /**
   * Resolves once the child process is gone — normal exit, crash, kill, or
   * spawn failure. Never rejects. The lifecycle watches this to observe a
   * post-ready crash and to sequence a replacement AFTER real termination.
   */
  readonly terminated: Promise<void>;
  /** True while the child process is alive. */
  isRunning(): boolean;
  /** Kill the child (idempotent, does not wait). */
  dispose(): void;
  /**
   * Kill the child and wait for it to actually be gone, escalating to
   * SIGKILL after `escalateAfterMs` for a child that traps SIGTERM.
   */
  terminate(escalateAfterMs?: number): Promise<void>;
}

export interface McpServerSpawnOptions {
  readonly command: string;
  readonly args: readonly string[];
  readonly log: McpServerLog;
  /** How long the child gets to print its readiness record. */
  readonly readyTimeoutMs?: number;
}

const DEFAULT_READY_TIMEOUT_MS = 60_000;

/**
 * Spawn a standalone MCP server process and watch its stdout for the
 * readiness record. The returned handle owns the child: disposal kills it,
 * and a launch that never becomes ready is killed rather than leaked.
 */
export function startMcpServerProcess(options: McpServerSpawnOptions): McpServerProcessHandle {
  const readyTimeoutMs = options.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS;
  const { log } = options;

  let running = false;
  let disposed = false;
  let settled = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  let resolveReady!: (record: McpHttpReadyRecord) => void;
  let rejectReady!: (error: Error) => void;
  const ready = new Promise<McpHttpReadyRecord>((resolvePromise, rejectPromise) => {
    resolveReady = resolvePromise;
    rejectReady = rejectPromise;
  });
  // The extension observes readiness failures through its own handler; this
  // guard only keeps an undelivered rejection from crashing the host.
  ready.catch(() => {});

  const settle = (outcome: () => void) => {
    if (settled) return;
    settled = true;
    if (timer) {
      clearTimeout(timer);
      timer = undefined;
    }
    outcome();
  };

  let resolveTerminated!: () => void;
  const terminated = new Promise<void>((resolvePromise) => {
    resolveTerminated = resolvePromise;
  });

  const child: ChildProcess = spawn(options.command, [...options.args], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  running = true;

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    // Settle FIRST so disposal leaves neither an armed readiness timer nor a
    // pending promise behind (the start-attempt lifetime contract: a disposed
    // attempt owns nothing afterwards).
    settle(() => rejectReady(new Error("MCP server launch was disposed")));
    if (running) {
      try {
        child.kill();
      } catch {
        // Already gone.
      }
    }
  };

  timer = setTimeout(() => {
    settle(() => {
      rejectReady(new Error(`no readiness record on stdout within ${readyTimeoutMs}ms`));
    });
    // A child that never announced readiness has no discoverable port — kill
    // it rather than leak an unreachable server.
    dispose();
  }, readyTimeoutMs);

  child.on("error", (error) => {
    // A spawn-level error means the process never ran (or is unkillable);
    // there is no exit event to wait for.
    running = false;
    resolveTerminated();
    settle(() => rejectReady(error instanceof Error ? error : new Error(String(error))));
  });

  child.on("exit", (code, signal) => {
    running = false;
    resolveTerminated();
    settle(() =>
      rejectReady(new Error(`server exited before readiness (code=${code}, signal=${signal})`)),
    );
  });

  if (child.stdout) {
    const stdoutLines = createInterface({ input: child.stdout });
    stdoutLines.on("line", (line) => {
      const record = parseMcpHttpReadyRecord(line);
      if (record) {
        settle(() => resolveReady(record));
      }
    });
  }
  if (child.stderr) {
    // Human tracing — useful diagnostics, never port identity.
    const stderrLines = createInterface({ input: child.stderr });
    stderrLines.on("line", (line) => log.debug(`[verter-mcp] ${line}`));
  }

  const terminate = async (escalateAfterMs = 3_000) => {
    dispose();
    if (!running) {
      return terminated;
    }
    // SIGTERM was sent by dispose; a child that traps it gets SIGKILL. The
    // timer is cleared as soon as the exit lands, so a completed terminate
    // leaves nothing armed.
    const escalation = setTimeout(() => {
      try {
        child.kill("SIGKILL");
      } catch {
        // Already gone.
      }
    }, escalateAfterMs);
    try {
      await terminated;
    } finally {
      clearTimeout(escalation);
    }
  };

  return {
    ready,
    terminated,
    isRunning: () => running,
    dispose,
    terminate,
  };
}

// ---------------------------------------------------------------------------
// Lifecycle supervisor
// ---------------------------------------------------------------------------

/** Why a previously-serving MCP server is no longer serving. */
export type McpStopReason = "crash" | "replaced" | "disabled";

export interface McpLifecycleEvents {
  /** The current server announced readiness (register the provider here). */
  onReady(record: McpHttpReadyRecord): void;
  /**
   * A server that HAD announced readiness stopped serving — it crashed, was
   * replaced by a config change, or was disabled. Tear down the provider
   * registration here: a dead URL must not stay registered.
   */
  onStopped(reason: McpStopReason): void;
}

export interface McpServerLifecycleOptions {
  readonly log: McpServerLog;
  /** Resolve the server binary at spawn time (config may change between spawns). */
  readonly resolveBinary: () => McpBinaryCandidate | undefined;
  /** Spawn seam — production uses {@link startMcpServerProcess}. */
  readonly startProcess?: (
    binary: McpBinaryCandidate,
    config: McpLaunchConfig,
  ) => McpServerProcessHandle;
  /**
   * How many unexpected exits are answered with a respawn before giving up.
   * The budget resets on a config change, never on readiness — a
   * ready/crash/ready loop stays bounded.
   */
  readonly maxCrashRespawns?: number;
  readonly events: McpLifecycleEvents;
}

/** The supervisor owning the standalone server across one LSP start attempt. */
export interface McpServerLifecycle {
  /** Reconcile toward the desired config (`undefined` = disabled). Serialized. */
  sync(desired: McpLaunchConfig | undefined): void;
  /** Kill everything, immediately and permanently (attempt teardown). */
  dispose(): void;
  /** Resolves when every queued reconciliation has settled (test hook). */
  settled(): Promise<void>;
}

const DEFAULT_MAX_CRASH_RESPAWNS = 3;

/**
 * Supervise the standalone MCP server: spawn on demand, replace on config
 * change (awaiting REAL termination of the predecessor so two children are
 * never alive at once), observe post-ready crashes, tear the consumer down
 * through `onStopped`, and respawn within a bounded budget.
 */
export function createMcpServerLifecycle(options: McpServerLifecycleOptions): McpServerLifecycle {
  const { log, events } = options;
  const startProcess =
    options.startProcess ??
    ((binary: McpBinaryCandidate, config: McpLaunchConfig) =>
      startMcpServerProcess({ command: binary.path, args: buildMcpServerArgs(config), log }));
  const maxCrashRespawns = options.maxCrashRespawns ?? DEFAULT_MAX_CRASH_RESPAWNS;

  let disposed = false;
  let chain: Promise<void> = Promise.resolve();
  let currentHandle: McpServerProcessHandle | undefined;
  let currentKey: string | undefined;
  let currentReady = false;
  let crashRespawns = 0;

  const enqueue = (task: () => Promise<void>) => {
    chain = chain.then(task).catch((error) => {
      log.warn(`MCP server lifecycle step failed: ${error}`);
    });
  };

  const spawn = (config: McpLaunchConfig) => {
    const binary = options.resolveBinary();
    if (!binary) {
      log.warn(
        "Standalone verter-mcp binary not found; MCP tools are unavailable. " +
          "Build it with `cargo build -p verter_mcp` or disable `verter.mcp.enabled`.",
      );
      return;
    }
    log.info(`MCP server binary: ${binary.path} (${binary.source})`);
    const handle = startProcess(binary, config);
    currentHandle = handle;
    currentReady = false;

    void handle.ready
      .then((record) => {
        if (disposed || currentHandle !== handle) return;
        currentReady = true;
        events.onReady(record);
      })
      .catch((error) => {
        if (disposed || currentHandle !== handle) return;
        log.warn(`Standalone MCP server did not become ready: ${error}`);
      });

    // Crash watch: an exit while this handle is still current was not asked
    // for. Report it, and respawn within the bounded budget.
    void handle.terminated.then(() => {
      if (disposed || currentHandle !== handle) return;
      const wasReady = currentReady;
      currentHandle = undefined;
      currentReady = false;
      if (wasReady) {
        events.onStopped("crash");
      }
      if (crashRespawns < maxCrashRespawns) {
        crashRespawns += 1;
        log.warn(
          `Standalone MCP server exited unexpectedly; respawning ` +
            `(${crashRespawns}/${maxCrashRespawns})`,
        );
        enqueue(async () => {
          if (disposed || currentHandle !== undefined || currentKey !== JSON.stringify(config)) {
            return;
          }
          spawn(config);
        });
      } else {
        log.warn(
          "Standalone MCP server exited unexpectedly and the respawn budget is " +
            "exhausted; MCP stays down until settings change or the server restarts.",
        );
      }
    });
  };

  const reconcile = async (desired: McpLaunchConfig | undefined) => {
    if (disposed) return;
    const desiredKey = desired ? JSON.stringify(desired) : undefined;
    if (
      desiredKey === currentKey &&
      (desired === undefined || (currentHandle !== undefined && currentHandle.isRunning()))
    ) {
      return;
    }
    if (desiredKey !== currentKey) {
      crashRespawns = 0;
    }

    const previous = currentHandle;
    const previousWasReady = currentReady;
    currentHandle = undefined;
    currentReady = false;
    currentKey = desiredKey;
    if (previous) {
      // The replacement may reuse a fixed port: wait until the predecessor is
      // REALLY gone, or the new bind races the dying listener.
      await previous.terminate();
      if (!disposed && previousWasReady) {
        events.onStopped(desired ? "replaced" : "disabled");
      }
    }
    if (disposed || !desired) return;
    spawn(desired);
  };

  return {
    sync(desired) {
      enqueue(() => reconcile(desired));
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      currentHandle?.dispose();
      currentHandle = undefined;
      currentReady = false;
    },
    async settled() {
      // Crash watches append to the chain from their own microtasks, so a
      // single snapshot can resolve before a just-triggered reconciliation is
      // even queued. Drain until the chain is stable.
      let snapshot: Promise<void>;
      do {
        snapshot = chain;
        await snapshot;
      } while (snapshot !== chain);
    },
  };
}

// ---------------------------------------------------------------------------
// Setup-command endpoint resolution
// ---------------------------------------------------------------------------

export interface McpSetupEndpointOptions {
  /** The live `verter.mcp.enabled` value at command time. */
  readonly mcpEnabled: boolean;
  /** The current ready endpoint, if the standalone server is already up. */
  readonly getEndpoint: () => McpHttpReadyRecord | undefined;
  /**
   * Kick the language-server start attempt that owns the MCP lifecycle.
   * Command-triggered activation with no carrier open never started it, so
   * the setup command must — otherwise nothing will ever become ready.
   */
  readonly ensureStarted: () => void;
  readonly timeoutMs?: number;
  readonly pollMs?: number;
}

export type McpSetupEndpointResult = { readonly url: string } | { readonly refusal: string };

const DEFAULT_SETUP_READY_TIMEOUT_MS = 30_000;
const DEFAULT_SETUP_POLL_MS = 250;

/**
 * Resolve the LIVE endpoint the setup command may write into `.mcp.json`.
 *
 * Never yields a dead endpoint: when the server is not running it is started
 * and awaited (bounded); when it cannot become ready — or MCP is disabled —
 * the result is a refusal for the user, and nothing gets written.
 */
export async function resolveMcpEndpointForSetup(
  options: McpSetupEndpointOptions,
): Promise<McpSetupEndpointResult> {
  // The disabled check DOMINATES the cache: after `verter.mcp.enabled` flips
  // off, the debounced restart tears the server down shortly after — but a
  // still-cached endpoint inside that window would otherwise be handed out
  // and persisted as a permanently dead URL.
  if (!options.mcpEnabled) {
    return {
      refusal:
        "Verter MCP is disabled (`verter.mcp.enabled` is false). Enable it, then run " +
        "“Verter: Setup MCP for Claude Code” again.",
    };
  }
  const existing = options.getEndpoint();
  if (existing) {
    return { url: existing.url };
  }

  options.ensureStarted();

  const timeoutMs = options.timeoutMs ?? DEFAULT_SETUP_READY_TIMEOUT_MS;
  const pollMs = options.pollMs ?? DEFAULT_SETUP_POLL_MS;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const endpoint = options.getEndpoint();
    if (endpoint) {
      return { url: endpoint.url };
    }
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  return {
    refusal:
      "The Verter MCP server did not become ready, so .mcp.json was not written " +
      "(a dead endpoint would break Claude Code). Check the Verter output channel " +
      "and run “Verter: Setup MCP for Claude Code” again once the server is up.",
  };
}

/**
 * The `verter.setupMcpForClaudeCode` command's dependency wiring, extracted
 * from extension.ts so it is unit-provable. Two properties live here:
 *
 * - COLD path: with no LSP running (command-triggered activation, no carrier
 *   open), the command itself kicks {@link McpSetupCommandDeps.ensureLanguageServerStarted}
 *   — nothing else will ever produce a ready endpoint.
 * - RETRY path: with the LSP already running, `ensureLanguageServerStarted`
 *   returns immediately, so a FAILED MCP child (missing binary, exhausted
 *   crash-respawn budget) would stay down forever. An explicit Setup click is
 *   a user retry: {@link McpSetupCommandDeps.retryMcpLifecycleSync} re-syncs
 *   the live attempt's MCP lifecycle so a fixed cause gets a fresh spawn.
 */
export interface McpSetupCommandDeps {
  /** Read the LIVE `verter.mcp.enabled` value at command time. */
  readonly readMcpEnabled: () => boolean;
  /** The current ready endpoint, if the standalone server is already up. */
  readonly getEndpoint: () => McpHttpReadyRecord | undefined;
  /** Start (or join) the LSP attempt that owns the MCP lifecycle. */
  readonly ensureLanguageServerStarted: () => Promise<unknown>;
  /** Re-sync the live attempt's MCP lifecycle; a no-op when none is live. */
  readonly retryMcpLifecycleSync: () => void;
  /** Persist the resolved LIVE url (the `.mcp.json` write). */
  readonly writeSetup: (url: string) => void;
  /** Surface a refusal to the user; nothing has been written. */
  readonly refuse: (message: string) => void;
  readonly timeoutMs?: number;
  readonly pollMs?: number;
}

export async function runMcpSetupCommand(deps: McpSetupCommandDeps): Promise<void> {
  const result = await resolveMcpEndpointForSetup({
    mcpEnabled: deps.readMcpEnabled(),
    getEndpoint: deps.getEndpoint,
    ensureStarted: () => {
      // Retry FIRST (synchronous): when an attempt is live its lifecycle may
      // hold a failed MCP child that only a re-sync can respawn. Then kick
      // the LSP start — a no-op when it already runs, the sole readiness
      // producer when it does not.
      deps.retryMcpLifecycleSync();
      void deps.ensureLanguageServerStarted().catch(() => {
        // A failed LSP start surfaces through its own error paths; the
        // bounded endpoint wait in the resolver turns it into a refusal.
      });
    },
    timeoutMs: deps.timeoutMs,
    pollMs: deps.pollMs,
  });
  if ("refusal" in result) {
    deps.refuse(result.refusal);
    return;
  }
  deps.writeSetup(result.url);
}
