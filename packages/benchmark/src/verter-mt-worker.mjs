/**
 * Verter multi-threaded worker.
 * Each worker creates its own VerterHost and compiles a subset of files.
 * Used by apple-to-apple.ts for the multi-threaded stress test.
 */
import { workerData, parentPort } from "worker_threads";
import { createRequire } from "module";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// Resolve @verter/native from the benchmark package's node_modules
const _require = createRequire(join(__dirname, "../package.json"));
const { VerterHost } = _require("@verter/native");

const host = new VerterHost({ devMode: false, analysisLevel: "none" });
const hostProfile = { sourceMap: false };
const { files } = workerData;

let compiled = 0;
for (const { filename, source } of files) {
  try {
    host.remove(filename);
    const result = host.upsert({ inputId: filename, source, compileProfile: hostProfile });
    host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "script" },
      compileProfile: hostProfile,
    });
    host.getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: "template" },
      compileProfile: hostProfile,
    });
    compiled++;
  } catch {
    // ignore errors
  }
}

parentPort?.postMessage(compiled);
