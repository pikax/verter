import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { getMetaRuntime, shutdownMetaRuntime } from "./meta-runtime.js";
import { ProjectEngine } from "./project-engine.js";
import type { NativeMetaProject, NativeMetaSession } from "./project-engine.js";
import type { EngineKeyInput } from "./engine-key.js";

/** Minimal mock NativeMetaSession. */
function mockNativeSession(): NativeMetaSession {
  return {
    upsert() {},
    delete() {},
    getComponentMeta() {
      return null;
    },
    getEffectiveSource() {
      return null;
    },
    hasFile() {
      return false;
    },
    trackedFileIds() {
      return [];
    },
    close() {},
    get isClosed() {
      return false;
    },
    get overlayGeneration() {
      return 0;
    },
  };
}

/** Minimal mock NativeMetaProject. */
function mockNativeProject(): NativeMetaProject {
  let _shutdown = false;
  return {
    upsertBase() {},
    ensureLoaded() {
      return false;
    },
    refreshBase() {
      return false;
    },
    configureProjects() {},
    openSession: () => mockNativeSession(),
    clearCaches() {},
    shutdown() {
      _shutdown = true;
    },
    get isShutdown() {
      return _shutdown;
    },
    get sessionCount() {
      return 0;
    },
    baseFileIds: () => [],
  };
}

function baseInput(root = "/project"): EngineKeyInput {
  return {
    backend: "napi",
    root,
    configKind: "tsconfig",
    tsconfigPath: `${root}/tsconfig.json`,
    configHash: "hash1",
    nativeFlags: { analysisLevel: "full", deepMacroResolutionType: true },
  };
}

describe("MetaRuntime", () => {
  let runtime: ReturnType<typeof getMetaRuntime>;

  beforeEach(() => {
    shutdownMetaRuntime();
    runtime = getMetaRuntime();
  });

  afterEach(() => {
    shutdownMetaRuntime();
  });

  it("same key reuses one engine", async () => {
    const input = baseInput();
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine1 = await runtime.getOrCreateEngine(input, bootstrap);
    const engine2 = await runtime.getOrCreateEngine(input, bootstrap);
    expect(engine1).toBe(engine2);
    expect(runtime.diagnostics.enginesCreated).toBe(1);
    expect(runtime.diagnostics.enginesReused).toBe(1);
  });

  it("different config hash creates different engines", async () => {
    const input1 = baseInput();
    const input2 = { ...baseInput(), configHash: "hash2" };
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine1 = await runtime.getOrCreateEngine(input1, bootstrap);
    const engine2 = await runtime.getOrCreateEngine(input2, bootstrap);
    expect(engine1).not.toBe(engine2);
    expect(runtime.diagnostics.enginesCreated).toBe(2);
  });

  it("parallel opens for the same key dedup to one engine", async () => {
    const input = baseInput();
    let bootstrapCalls = 0;
    const bootstrap = async () => {
      bootstrapCalls++;
      await new Promise((r) => setTimeout(r, 10));
      return { nativeProject: mockNativeProject(), baseFileIds: [] };
    };

    const [e1, e2] = await Promise.all([
      runtime.getOrCreateEngine(input, bootstrap),
      runtime.getOrCreateEngine(input, bootstrap),
    ]);
    expect(e1).toBe(e2);
    expect(bootstrapCalls).toBe(1);
  });

  it("open session acquires lease and can be closed", async () => {
    const input = baseInput();
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    expect(engine.leaseCount).toBe(1);

    runtime.closeSession(session);
    expect(engine.leaseCount).toBe(0);
    expect(session.closed).toBe(true);
    expect(runtime.diagnostics.closeCalls).toBe(1);
  });

  it("closed session methods throw", async () => {
    const input = baseInput();
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const session = runtime.openSession(engine);
    runtime.closeSession(session);

    expect(() => session.upsert("test.vue", "source")).toThrow("Session is closed");
    expect(() => session.delete("test.vue")).toThrow("Session is closed");
    expect(() => session.getComponentMeta("test.vue")).toThrow("Session is closed");
  });

  it("evictEngine shuts down and removes engine", async () => {
    const input = baseInput();
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine = await runtime.getOrCreateEngine(input, bootstrap);
    const key = engine.key;
    runtime.evictEngine(key);

    expect(engine.state).toBe("closed");
    expect(runtime.engineCount).toBe(0);
    expect(runtime.diagnostics.enginesForceEvicted).toBe(1);
  });

  it("shutdownMetaRuntime closes all engines", async () => {
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    await runtime.getOrCreateEngine(baseInput("/a"), bootstrap);
    await runtime.getOrCreateEngine(baseInput("/b"), bootstrap);

    expect(runtime.engineCount).toBe(2);
    shutdownMetaRuntime();
    // After shutdown, the singleton is reset
    const fresh = getMetaRuntime();
    expect(fresh.engineCount).toBe(0);
  });

  it("open after shutdown creates fresh runtime", async () => {
    shutdownMetaRuntime();
    const fresh = getMetaRuntime();
    expect(fresh.shuttingDown).toBe(false);

    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });
    const engine = await fresh.getOrCreateEngine(baseInput(), bootstrap);
    expect(engine.state).toBe("active");
    shutdownMetaRuntime();
  });

  it("eviction skips engines with live leases", async () => {
    const bootstrap = async () => ({
      nativeProject: mockNativeProject(),
      baseFileIds: [],
    });

    const engine = await runtime.getOrCreateEngine(baseInput(), bootstrap);
    const session = runtime.openSession(engine);

    // Can't shutdown without force
    expect(engine.shutdownNow(false)).toBe(false);
    expect(engine.state).toBe("active");

    // Cleanup
    runtime.closeSession(session);
    expect(engine.shutdownNow(false)).toBe(true);
  });

  it("bootstrap failure surfaces error and allows retry", async () => {
    let calls = 0;
    const bootstrap = async () => {
      calls++;
      if (calls === 1) throw new Error("bootstrap failed");
      return { nativeProject: mockNativeProject(), baseFileIds: [] };
    };

    await expect(runtime.getOrCreateEngine(baseInput(), bootstrap)).rejects.toThrow(
      "bootstrap failed",
    );
    // Retry succeeds
    const engine = await runtime.getOrCreateEngine(baseInput(), bootstrap);
    expect(engine.state).toBe("active");
  });
});
