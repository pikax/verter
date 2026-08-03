// Unit coverage for the standalone verter-mcp launch/readiness module.
//
// The E2E acceptance ("standalone MCP server reports a valid bound port" /
// "MCP server registered with VS Code" in e2e/suite/activation.test.ts) proves
// the wired path against the real binary; these tests pin the pieces that can
// break independently: the cross-language readiness-record encoding, the child
// argv, the binary candidate ordering, and the spawn/readiness lifecycle.

import { describe, expect, it, vi } from "vitest";

import {
  buildMcpServerArgs,
  bundledMcpBinaryCandidate,
  createMcpServerLifecycle,
  orderedMcpBinaryCandidates,
  parseMcpHttpReadyRecord,
  resolveMcpEndpointForSetup,
  runMcpSetupCommand,
  startMcpServerProcess,
  type McpHttpReadyRecord,
  type McpLaunchConfig,
  type McpServerProcessHandle,
} from "./mcpServer";

/**
 * The exact sample also pinned by the Rust encoder's unit test
 * (`crates/verter_mcp/src/readiness.rs`). Changing the encoding requires
 * changing both pins in the same commit.
 */
const CROSS_LANGUAGE_SAMPLE =
  '{"verterMcpHttpReady":{"port":54321,"url":"http://127.0.0.1:54321/mcp"}}';

const silentLog = { info() {}, warn() {}, debug() {} };

describe("parseMcpHttpReadyRecord", () => {
  it("parses the cross-language pinned sample", () => {
    expect(parseMcpHttpReadyRecord(CROSS_LANGUAGE_SAMPLE)).toEqual({
      port: 54321,
      url: "http://127.0.0.1:54321/mcp",
    });
  });

  it("accepts surrounding whitespace only", () => {
    expect(parseMcpHttpReadyRecord(`  ${CROSS_LANGUAGE_SAMPLE}\n`)).toEqual({
      port: 54321,
      url: "http://127.0.0.1:54321/mcp",
    });
  });

  it("rejects noise, foreign JSON, and malformed records", () => {
    // Human tracing noise (stderr shape) is never port identity.
    expect(
      parseMcpHttpReadyRecord("2026-07-28 INFO Starting Verter MCP server (HTTP)"),
    ).toBeUndefined();
    expect(parseMcpHttpReadyRecord('{"jsonrpc":"2.0","id":1}')).toBeUndefined();
    expect(parseMcpHttpReadyRecord('{"ready":{"port":1,"url":"u"}}')).toBeUndefined();
    // Port 0 is not a bound port.
    expect(
      parseMcpHttpReadyRecord('{"verterMcpHttpReady":{"port":0,"url":"http://127.0.0.1:0/mcp"}}'),
    ).toBeUndefined();
    // Out-of-range and non-numeric ports.
    expect(
      parseMcpHttpReadyRecord(
        '{"verterMcpHttpReady":{"port":65536,"url":"http://127.0.0.1:65536/mcp"}}',
      ),
    ).toBeUndefined();
    expect(
      parseMcpHttpReadyRecord(
        '{"verterMcpHttpReady":{"port":"54321","url":"http://127.0.0.1:54321/mcp"}}',
      ),
    ).toBeUndefined();
    expect(parseMcpHttpReadyRecord("")).toBeUndefined();
  });
});

describe("buildMcpServerArgs", () => {
  it("builds the standalone HTTP argv with a project root", () => {
    expect(buildMcpServerArgs({ port: 0, lintPreset: "recommended", rootPath: "/ws/app" })).toEqual(
      [
        "--transport",
        "http",
        "--port",
        "0",
        "--lint-preset",
        "recommended",
        "--project-root",
        "/ws/app",
      ],
    );
  });

  it("omits --project-root when no workspace folder exists", () => {
    const args = buildMcpServerArgs({ port: 6772, lintPreset: "strict" });
    expect(args).toContain("--port");
    expect(args[args.indexOf("--port") + 1]).toBe("6772");
    expect(args).not.toContain("--project-root");
  });

  it("never emits the LSP's retired --mcp-port flag", () => {
    const args = buildMcpServerArgs({ port: 0, lintPreset: "recommended", rootPath: "/ws" });
    expect(args.join(" ")).not.toContain("--mcp-port");
  });

  it("binds the server lifetime to the host with --client-pid", () => {
    const args = buildMcpServerArgs({
      port: 0,
      lintPreset: "recommended",
      rootPath: "/ws",
      clientPid: 4321,
    });
    expect(args).toContain("--client-pid");
    expect(args[args.indexOf("--client-pid") + 1]).toBe("4321");
    // Without a pid the flag is absent (standalone agent launches own their lifetime).
    expect(buildMcpServerArgs({ port: 0, lintPreset: "recommended" })).not.toContain(
      "--client-pid",
    );
  });
});

describe("binary candidate ordering", () => {
  const launcherCandidates = [
    { path: "/pkg/@verter/mcp-host/verter-mcp", source: "platform-package" },
    { path: "/repo/target/debug/verter-mcp", source: "dev-build" },
    { path: "/repo/target/release/verter-mcp", source: "dev-build" },
    { path: "verter-mcp", source: "path" },
  ] as const;

  it("slots the bundled VSIX binary between dev builds and the PATH fallback", () => {
    const bundled = bundledMcpBinaryCandidate("/ext", "linux");
    const ordered = orderedMcpBinaryCandidates(launcherCandidates, bundled);
    expect(ordered.map((candidate) => candidate.source)).toEqual([
      "platform-package",
      "dev-build",
      "dev-build",
      "bundled",
      "path",
    ]);
    expect(ordered[3].path).toBe("/ext/bin/verter-mcp");
  });

  it("appends the bundled binary when the launcher offers no PATH fallback", () => {
    const ordered = orderedMcpBinaryCandidates([], bundledMcpBinaryCandidate("/ext", "linux"));
    expect(ordered.map((candidate) => candidate.source)).toEqual(["bundled"]);
  });

  // @ai-generated - Pins target-platform path semantics independently of the host OS.
  it("uses the requested platform's exact path dialect", () => {
    expect(bundledMcpBinaryCandidate("C:\\ext", "win32").path).toBe("C:\\ext\\bin\\verter-mcp.exe");
    expect(bundledMcpBinaryCandidate("/ext", "darwin").path).toBe("/ext/bin/verter-mcp");
    expect(bundledMcpBinaryCandidate("/ext", "linux").path).toBe("/ext/bin/verter-mcp");
  });
});

describe("startMcpServerProcess", () => {
  const fakeServer = (script: string) => ({
    command: process.execPath,
    args: ["-e", script],
  });

  it("resolves readiness from the record line, ignoring stdout noise", async () => {
    const { command, args } = fakeServer(
      `console.log("startup banner");
       console.log(JSON.stringify({ verterMcpHttpReady: { port: 43210, url: "http://127.0.0.1:43210/mcp" } }));
       setInterval(() => {}, 1000);`,
    );
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    try {
      await expect(handle.ready).resolves.toEqual({
        port: 43210,
        url: "http://127.0.0.1:43210/mcp",
      });
    } finally {
      handle.dispose();
    }
  });

  it("rejects when the child exits before announcing readiness", async () => {
    const { command, args } = fakeServer(`console.log("no record here"); process.exit(3);`);
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    try {
      await expect(handle.ready).rejects.toThrow(/exited/);
    } finally {
      handle.dispose();
    }
  });

  it("rejects when the child cannot be spawned at all", async () => {
    const handle = startMcpServerProcess({
      command: "/nonexistent/verter-mcp-binary",
      args: [],
      log: silentLog,
      readyTimeoutMs: 15_000,
    });
    try {
      await expect(handle.ready).rejects.toThrow();
    } finally {
      handle.dispose();
    }
  });

  it("rejects on readiness timeout and kills the child", async () => {
    const { command, args } = fakeServer(`setInterval(() => {}, 1000);`);
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 250 });
    try {
      await expect(handle.ready).rejects.toThrow(/readiness/i);
    } finally {
      handle.dispose();
    }
    // The child must not outlive the rejected launch.
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(handle.isRunning()).toBe(false);
  });

  it("dispose before readiness disarms the timeout and rejects ready", async () => {
    // The start-attempt lifetime contract: a disposed attempt owns nothing
    // afterwards — no armed timer, no pending promise, no live child.
    vi.useFakeTimers();
    try {
      const { command, args } = fakeServer(`setInterval(() => {}, 1000);`);
      const handle = startMcpServerProcess({
        command,
        args,
        log: silentLog,
        readyTimeoutMs: 60_000,
      });
      expect(vi.getTimerCount()).toBe(1);
      handle.dispose();
      expect(vi.getTimerCount()).toBe(0);
      await expect(handle.ready).rejects.toThrow(/disposed/);
    } finally {
      vi.useRealTimers();
    }
  });

  it("dispose kills a ready child", async () => {
    const { command, args } = fakeServer(
      `console.log(JSON.stringify({ verterMcpHttpReady: { port: 43211, url: "http://127.0.0.1:43211/mcp" } }));
       setInterval(() => {}, 1000);`,
    );
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    await handle.ready;
    handle.dispose();
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(handle.isRunning()).toBe(false);
  });

  it("terminated resolves when a ready child exits on its own (crash observation)", async () => {
    // The child announces readiness, then dies 100ms later.
    const { command, args } = fakeServer(
      `console.log(JSON.stringify({ verterMcpHttpReady: { port: 43212, url: "http://127.0.0.1:43212/mcp" } }));
       setTimeout(() => process.exit(7), 100);`,
    );
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    await handle.ready;
    await handle.terminated;
    expect(handle.isRunning()).toBe(false);
  });

  it("terminated resolves even when the child could never be spawned", async () => {
    const handle = startMcpServerProcess({
      command: "/nonexistent/verter-mcp-binary",
      args: [],
      log: silentLog,
      readyTimeoutMs: 15_000,
    });
    await expect(handle.ready).rejects.toThrow();
    await handle.terminated;
    expect(handle.isRunning()).toBe(false);
  });

  it("terminate awaits real child exit so a replacement can never overlap", async () => {
    const { command, args } = fakeServer(
      `console.log(JSON.stringify({ verterMcpHttpReady: { port: 43213, url: "http://127.0.0.1:43213/mcp" } }));
       setInterval(() => {}, 1000);`,
    );
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    await handle.ready;
    await handle.terminate();
    // No settling grace period: once terminate resolves the child is GONE.
    expect(handle.isRunning()).toBe(false);
  });

  it("terminate escalates to SIGKILL for a child that traps SIGTERM", async () => {
    const { command, args } = fakeServer(
      `process.on("SIGTERM", () => {});
       console.log(JSON.stringify({ verterMcpHttpReady: { port: 43214, url: "http://127.0.0.1:43214/mcp" } }));
       setInterval(() => {}, 1000);`,
    );
    const handle = startMcpServerProcess({ command, args, log: silentLog, readyTimeoutMs: 15_000 });
    await handle.ready;
    await handle.terminate(300);
    expect(handle.isRunning()).toBe(false);
  });
});

describe("resolveMcpEndpointForSetup", () => {
  const record: McpHttpReadyRecord = { port: 43299, url: "http://127.0.0.1:43299/mcp" };

  it("returns the live endpoint immediately without starting anything", async () => {
    let ensureCalls = 0;
    const result = await resolveMcpEndpointForSetup({
      mcpEnabled: true,
      getEndpoint: () => record,
      ensureStarted: () => {
        ensureCalls += 1;
      },
    });
    expect(result).toEqual({ url: record.url });
    expect(ensureCalls).toBe(0);
  });

  it("refuses when verter.mcp.enabled is off, naming the setting", async () => {
    let ensureCalls = 0;
    const result = await resolveMcpEndpointForSetup({
      mcpEnabled: false,
      getEndpoint: () => undefined,
      ensureStarted: () => {
        ensureCalls += 1;
      },
    });
    expect("refusal" in result && result.refusal).toContain("verter.mcp.enabled");
    expect(ensureCalls).toBe(0);
  });

  it("refuses when disabled EVEN IF a live endpoint is still cached", async () => {
    // The debounce window: `verter.mcp.enabled` was just flipped off, the
    // 200ms restart debounce has not yet torn the server down, so a live
    // endpoint is still cached. Handing it out would let setup persist a
    // URL the scheduled restart is about to kill — the disabled check must
    // dominate whatever is cached.
    let ensureCalls = 0;
    const result = await resolveMcpEndpointForSetup({
      mcpEnabled: false,
      getEndpoint: () => record,
      ensureStarted: () => {
        ensureCalls += 1;
      },
    });
    expect("url" in result, "a disabled MCP must never yield a writable URL").toBe(false);
    expect("refusal" in result && result.refusal).toContain("verter.mcp.enabled");
    expect(ensureCalls).toBe(0);
  });

  it("starts the server and waits for readiness when nothing is running", async () => {
    // The M2 scenario: command-triggered activation with no carrier open —
    // nothing is running until the command itself starts the lifecycle.
    let ensureCalls = 0;
    let endpoint: McpHttpReadyRecord | undefined;
    const result = await resolveMcpEndpointForSetup({
      mcpEnabled: true,
      getEndpoint: () => endpoint,
      ensureStarted: () => {
        ensureCalls += 1;
        setTimeout(() => {
          endpoint = record;
        }, 60);
      },
      pollMs: 20,
      timeoutMs: 2_000,
    });
    expect(result).toEqual({ url: record.url });
    expect(ensureCalls).toBe(1);
  });

  it("refuses (never writes a dead endpoint) when the server never becomes ready", async () => {
    const result = await resolveMcpEndpointForSetup({
      mcpEnabled: true,
      getEndpoint: () => undefined,
      ensureStarted: () => {},
      pollMs: 20,
      timeoutMs: 150,
    });
    expect("refusal" in result && result.refusal).toMatch(/not written|did not become ready/i);
  });
});

// The setup COMMAND's dependency wiring, extracted from extension.ts so the
// cold path is provable: the E2E suite warms the LSP before any test runs, so
// "invoking Setup when the server is not yet running still writes a live
// endpoint" is covered HERE, not by e2e/suite/activation.test.ts (which only
// exercises the warm, endpoint-already-cached path).
describe("runMcpSetupCommand", () => {
  const record: McpHttpReadyRecord = { port: 43301, url: "http://127.0.0.1:43301/mcp" };

  interface Trace {
    ensureCalls: number;
    retryCalls: number;
    written: string[];
    refusals: string[];
  }

  function makeDeps(overrides: Partial<Parameters<typeof runMcpSetupCommand>[0]> = {}) {
    const trace: Trace = { ensureCalls: 0, retryCalls: 0, written: [], refusals: [] };
    let endpoint: McpHttpReadyRecord | undefined;
    const deps: Parameters<typeof runMcpSetupCommand>[0] = {
      readMcpEnabled: () => true,
      getEndpoint: () => endpoint,
      ensureLanguageServerStarted: async () => {
        trace.ensureCalls += 1;
      },
      retryMcpLifecycleSync: () => {
        trace.retryCalls += 1;
      },
      writeSetup: (url) => {
        trace.written.push(url);
      },
      refuse: (message) => {
        trace.refusals.push(message);
      },
      pollMs: 20,
      timeoutMs: 2_000,
      ...overrides,
    };
    return { deps, trace, setEndpoint: (r: McpHttpReadyRecord | undefined) => (endpoint = r) };
  }

  it("COLD path: no endpoint and no LSP — the command itself starts the server and writes the endpoint it produces", async () => {
    // The property the E2E cannot reach (its suiteSetup warms the LSP):
    // command-triggered activation with no carrier open. Nothing is running;
    // ONLY the ensureLanguageServerStarted kick can ever produce readiness.
    // Deleting that kick from runMcpSetupCommand turns this RED (timeout →
    // refusal, nothing written).
    const { deps, trace, setEndpoint } = makeDeps();
    await runMcpSetupCommand({
      ...deps,
      ensureLanguageServerStarted: async () => {
        trace.ensureCalls += 1;
        setTimeout(() => setEndpoint(record), 60);
      },
    });
    expect(trace.ensureCalls).toBe(1);
    expect(trace.written).toEqual([record.url]);
    expect(trace.refusals).toEqual([]);
  });

  it("an explicit Setup click re-syncs the live attempt's MCP lifecycle (retry after MCP failure)", async () => {
    // The LSP is already up (ensureLanguageServerStarted returns immediately,
    // producing nothing) but the MCP child failed — binary discovery miss or
    // exhausted respawn budget. ONLY the lifecycle re-sync can produce
    // readiness here: without it, Setup waits the full timeout and refuses
    // again even after the user fixed the underlying cause.
    const { deps, trace, setEndpoint } = makeDeps();
    await runMcpSetupCommand({
      ...deps,
      retryMcpLifecycleSync: () => {
        trace.retryCalls += 1;
        setTimeout(() => setEndpoint(record), 60);
      },
    });
    expect(trace.retryCalls).toBe(1);
    expect(trace.written).toEqual([record.url]);
    expect(trace.refusals).toEqual([]);
  });

  it("routes a refusal to refuse() and writes nothing", async () => {
    const { deps, trace } = makeDeps({ readMcpEnabled: () => false });
    await runMcpSetupCommand(deps);
    expect(trace.written).toEqual([]);
    expect(trace.refusals.length).toBe(1);
    expect(trace.refusals[0]).toContain("verter.mcp.enabled");
    expect(trace.ensureCalls).toBe(0);
    expect(trace.retryCalls).toBe(0);
  });

  it("a cached live endpoint short-circuits: no start kick, no lifecycle re-sync", async () => {
    const { deps, trace, setEndpoint } = makeDeps();
    setEndpoint(record);
    await runMcpSetupCommand(deps);
    expect(trace.written).toEqual([record.url]);
    expect(trace.ensureCalls).toBe(0);
    expect(trace.retryCalls).toBe(0);
  });
});

describe("createMcpServerLifecycle", () => {
  interface FakeHandle extends McpServerProcessHandle {
    resolveReady(record?: McpHttpReadyRecord): void;
    rejectReady(error: Error): void;
    exit(): void;
    disposeCalls: number;
    terminateCalls: number;
    releaseTerminate(): void;
  }

  function makeFakeHandle(port: number): FakeHandle {
    let readyResolve!: (r: McpHttpReadyRecord) => void;
    let readyReject!: (e: Error) => void;
    const ready = new Promise<McpHttpReadyRecord>((res, rej) => {
      readyResolve = res;
      readyReject = rej;
    });
    ready.catch(() => {});
    let terminatedResolve!: () => void;
    const terminated = new Promise<void>((res) => {
      terminatedResolve = res;
    });
    let running = true;
    let terminateRelease: (() => void) | undefined;
    const handle: FakeHandle = {
      ready,
      terminated,
      isRunning: () => running,
      dispose() {
        handle.disposeCalls += 1;
      },
      async terminate() {
        handle.terminateCalls += 1;
        // Termination completes only when the test releases it, so overlap
        // between the dying child and its replacement is observable.
        await new Promise<void>((res) => {
          terminateRelease = res;
        });
        running = false;
        terminatedResolve();
      },
      resolveReady(record?: McpHttpReadyRecord) {
        readyResolve(record ?? { port, url: `http://127.0.0.1:${port}/mcp` });
      },
      rejectReady(error: Error) {
        readyReject(error);
      },
      exit() {
        running = false;
        terminatedResolve();
      },
      releaseTerminate() {
        terminateRelease?.();
      },
      disposeCalls: 0,
      terminateCalls: 0,
    };
    return handle;
  }

  function makeLifecycle(
    overrides: {
      maxCrashRespawns?: number;
      resolveBinary?: () => { path: string; source: "dev-build" } | undefined;
    } = {},
  ) {
    const spawned: FakeHandle[] = [];
    const events: { readyPorts: number[]; stopped: string[] } = { readyPorts: [], stopped: [] };
    let nextPort = 40000;
    const lifecycle = createMcpServerLifecycle({
      log: silentLog,
      resolveBinary:
        overrides.resolveBinary ?? (() => ({ path: "/fake/verter-mcp", source: "dev-build" })),
      startProcess: () => {
        const handle = makeFakeHandle(nextPort++);
        spawned.push(handle);
        return handle;
      },
      maxCrashRespawns: overrides.maxCrashRespawns,
      events: {
        onReady: (record) => events.readyPorts.push(record.port),
        onStopped: (reason) => events.stopped.push(reason),
      },
    });
    return { lifecycle, spawned, events };
  }

  const config = (port: number): McpLaunchConfig => ({ port, lintPreset: "recommended" });

  it("spawns once for an unchanged live config", async () => {
    const { lifecycle, spawned } = makeLifecycle();
    lifecycle.sync(config(0));
    lifecycle.sync(config(0));
    await lifecycle.settled();
    expect(spawned.length).toBe(1);
  });

  it("replaces on config change and never overlaps children", async () => {
    const { lifecycle, spawned, events } = makeLifecycle();
    lifecycle.sync(config(0));
    await lifecycle.settled();
    spawned[0].resolveReady();
    await lifecycle.settled();

    lifecycle.sync(config(6772));
    // The old child's terminate is still pending — the replacement must not
    // have been spawned yet.
    await Promise.resolve();
    await Promise.resolve();
    expect(spawned[0].terminateCalls).toBe(1);
    expect(spawned.length).toBe(1);

    spawned[0].releaseTerminate();
    await lifecycle.settled();
    expect(spawned.length).toBe(2);
    expect(events.stopped).toEqual(["replaced"]);
  });

  it("disabling stops the child and reports 'disabled'", async () => {
    const { lifecycle, spawned, events } = makeLifecycle();
    lifecycle.sync(config(0));
    await lifecycle.settled();
    spawned[0].resolveReady();
    await lifecycle.settled();

    lifecycle.sync(undefined);
    await Promise.resolve();
    spawned[0].releaseTerminate();
    await lifecycle.settled();
    expect(events.stopped).toEqual(["disabled"]);
    expect(spawned.length).toBe(1);
  });

  it("a post-ready crash tears down (onStopped) and respawns, bounded", async () => {
    const { lifecycle, spawned, events } = makeLifecycle({ maxCrashRespawns: 2 });
    lifecycle.sync(config(0));
    await lifecycle.settled();

    // ready → crash, three times over: respawn, respawn, then STOP (cap 2).
    for (let round = 0; round < 3; round += 1) {
      const current = spawned[spawned.length - 1];
      current.resolveReady();
      await lifecycle.settled();
      current.exit();
      await lifecycle.settled();
    }
    expect(spawned.length).toBe(3); // initial + 2 bounded respawns, no 4th
    expect(events.stopped).toEqual(["crash", "crash", "crash"]);
    expect(events.readyPorts.length).toBe(3);
  });

  it("sync with an UNCHANGED config retries a spawn that failed binary discovery", async () => {
    // The setup command's retry substrate: `verter.mcp.enabled` never changed,
    // the binary was missing on the first sync, the user then built it. An
    // explicit re-sync with the SAME config must attempt discovery again
    // rather than treat the config as already reconciled.
    let binaryExists = false;
    const { lifecycle, spawned } = makeLifecycle({
      resolveBinary: () =>
        binaryExists ? { path: "/fake/verter-mcp", source: "dev-build" } : undefined,
    });
    lifecycle.sync(config(0));
    await lifecycle.settled();
    expect(spawned.length).toBe(0); // discovery failed, nothing spawned

    binaryExists = true;
    lifecycle.sync(config(0)); // SAME config — the setup-command retry path
    await lifecycle.settled();
    expect(spawned.length).toBe(1);
  });

  it("sync with an UNCHANGED config respawns after the crash-respawn budget is exhausted", async () => {
    const { lifecycle, spawned } = makeLifecycle({ maxCrashRespawns: 0 });
    lifecycle.sync(config(0));
    await lifecycle.settled();
    spawned[0].resolveReady();
    await lifecycle.settled();
    spawned[0].exit();
    await lifecycle.settled();
    expect(spawned.length).toBe(1); // budget 0: no automatic respawn

    lifecycle.sync(config(0)); // explicit user retry via the setup command
    await lifecycle.settled();
    expect(spawned.length).toBe(2);
  });

  it("a config change resets the crash-respawn budget", async () => {
    const { lifecycle, spawned } = makeLifecycle({ maxCrashRespawns: 1 });
    lifecycle.sync(config(0));
    await lifecycle.settled();
    spawned[0].resolveReady();
    await lifecycle.settled();
    spawned[0].exit();
    await lifecycle.settled();
    expect(spawned.length).toBe(2); // budget 1 consumed
    spawned[1].resolveReady();
    await lifecycle.settled();
    spawned[1].exit();
    await lifecycle.settled();
    expect(spawned.length).toBe(2); // budget exhausted

    lifecycle.sync(config(9999)); // config change resets the budget
    await lifecycle.settled();
    expect(spawned.length).toBe(3);
    spawned[2].resolveReady();
    await lifecycle.settled();
    spawned[2].exit();
    await lifecycle.settled();
    expect(spawned.length).toBe(4);
  });

  it("dispose stops everything and blocks further spawns", async () => {
    const { lifecycle, spawned } = makeLifecycle();
    lifecycle.sync(config(0));
    await lifecycle.settled();
    lifecycle.dispose();
    expect(spawned[0].disposeCalls).toBeGreaterThan(0);
    lifecycle.sync(config(0));
    await lifecycle.settled();
    expect(spawned.length).toBe(1);
  });

  it("a crash of a REPLACED child never triggers events or respawn", async () => {
    const { lifecycle, spawned, events } = makeLifecycle();
    lifecycle.sync(config(0));
    await lifecycle.settled();
    spawned[0].resolveReady();
    await lifecycle.settled();
    lifecycle.sync(config(1234));
    await Promise.resolve();
    spawned[0].releaseTerminate(); // old child exits as part of replacement
    await lifecycle.settled();
    expect(spawned.length).toBe(2);
    // Only the deliberate replacement is reported; the old child's exit is
    // not ALSO a crash.
    expect(events.stopped).toEqual(["replaced"]);
  });
});
