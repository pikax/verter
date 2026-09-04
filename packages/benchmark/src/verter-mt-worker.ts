/**
 * Verter multi-threaded worker.
 * Each worker creates its own VerterHost and compiles a subset of files.
 * Used by apple-to-apple.ts for the multi-threaded stress test.
 */
import { workerData, parentPort } from "node:worker_threads";
import { VerterHost } from "@verter/native";
import { vueRuntimeClientRequest } from "./compilers/verter.js";

const host = new VerterHost({ devMode: false, analysisLevel: "none" });
const { files } = workerData as { files: Array<{ filename: string; source: string }> };

let compiled = 0;
for (const { filename, source } of files) {
  try {
    host.remove(filename);
    const result = host.upsert({ inputId: filename, source });
    host.compileRequest(result.canonicalId, vueRuntimeClientRequest(result.canonicalId));
    compiled++;
  } catch {
    // ignore errors
  }
}

parentPort?.postMessage(compiled);
