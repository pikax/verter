import { runProbes } from "./probes.js";
import { nodeGlobalsPresent } from "./index.js";

function inspectNodeGlobals() {
  const globalRef = globalThis;
  return {
    process: typeof globalRef.process !== "undefined",
    require: typeof globalRef.require === "function",
    module: typeof globalRef.module !== "undefined",
    __dirname: typeof globalRef.__dirname !== "undefined",
  };
}

self.onmessage = async (event) => {
  const started = Date.now();
  try {
    const nodeGlobals = inspectNodeGlobals();
    if (nodeGlobalsPresent(nodeGlobals)) {
      self.postMessage({
        ok: false,
        error: "Node globals available in worker (BWH1-AC3)",
        nodeGlobals,
      });
      return;
    }

    const { wasmJsUrl, wasmUrl, tsSource, vueSource } = event.data;
    const wasm = await import(wasmJsUrl);
    await wasm.default(wasmUrl);
    const host = new wasm.VerterHost({ auditEnabled: true });
    const probed = runProbes(host, { tsSource, vueSource });
    self.postMessage({
      ok: true,
      nodeGlobals,
      operations: probed.operations,
      identities: probed.identities,
      startupMs: Date.now() - started,
      memory: inspectMemory(),
    });
  } catch (error) {
    self.postMessage({
      ok: false,
      error: error instanceof Error ? error.message : String(error),
      startupMs: Date.now() - started,
    });
  }
};

function inspectMemory() {
  const memory = globalThis.performance?.memory;
  if (memory == null) {
    return { retainedBytes: null, unavailable: "performance.memory is Chromium-only" };
  }
  return {
    retainedBytes: memory.usedJSHeapSize,
    totalBytes: memory.totalJSHeapSize,
    unavailable: null,
  };
}
