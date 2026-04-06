import { performance } from "node:perf_hooks";

import {
  getDefaultUiRoot,
  readComponentSourceForTrace,
  resolveComponentFile,
} from "./trace-component-resolver.js";
import { loadVerterCompatModule } from "./verter-compat.js";

const componentToken = process.argv[2];

if (!componentToken) {
  console.error("Usage: tsx src/_trace-component.ts <ComponentPathOrName>");
  process.exit(1);
}

const jsAuditEnabled = process.env.VERTER_JS_AUDIT === "1";

function maybeGc(): void {
  (globalThis as typeof globalThis & { gc?: () => void }).gc?.();
}

function formatMemoryUsage(): string {
  const usage = process.memoryUsage();
  const heapMb = Math.round(usage.heapUsed / 1024 / 1024);
  const rssMb = Math.round(usage.rss / 1024 / 1024);
  return `heap=${heapMb}MB rss=${rssMb}MB`;
}

const uiRoot = getDefaultUiRoot(import.meta.dirname);

let file: string;
try {
  file = resolveComponentFile(componentToken, { uiRoot }).replace(/\\/g, "/");
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`ERROR: ${message}`);
  process.exit(2);
}

const source = readComponentSourceForTrace(file);
maybeGc();
const heapBeforeSetup = jsAuditEnabled ? formatMemoryUsage() : null;
const setupStart = performance.now();
const compat = await loadVerterCompatModule();
const checker = await compat.createCheckerByJson(
  uiRoot.replace(/\\/g, "/"),
  {
    compilerOptions: { strict: true, jsx: "preserve" },
  },
  {
    forceUseTs: true,
    runtimeMode: "dedicated",
    typeExpansionBackend: "verter",
  },
);
const setupMs = Math.round(performance.now() - setupStart);
maybeGc();
const heapAfterSetup = jsAuditEnabled ? formatMemoryUsage() : null;

function roughSizeOfObject(obj: unknown): number {
  const seen = new WeakSet();
  function estimate(value: unknown): number {
    if (value === null || value === undefined) return 0;
    if (typeof value === "boolean") return 4;
    if (typeof value === "number") return 8;
    if (typeof value === "string") return (value as string).length * 2;
    if (typeof value !== "object") return 0;
    const o = value as Record<string, unknown>;
    if (seen.has(o)) return 0;
    seen.add(o);
    let size = 0;
    if (Array.isArray(o)) {
      for (const item of o) size += estimate(item);
    } else {
      for (const key of Object.keys(o)) {
        size += key.length * 2 + estimate(o[key]);
      }
    }
    return size;
  }
  return estimate(obj);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

// No in-process timeout — the parent process owns the hard timeout via SIGKILL.
// This child runs the query directly and exits when done.

try {
  checker.updateFile(file, source);
  maybeGc();
  const heapBeforeQuery = jsAuditEnabled ? formatMemoryUsage() : null;
  const start = performance.now();

  const meta = await checker.getComponentMeta(file);

  const durationMs = Math.round(performance.now() - start);
  maybeGc();
  const heapAfterQuery = jsAuditEnabled ? formatMemoryUsage() : null;
  const propsCount = meta?.props?.length ?? 0;
  if (jsAuditEnabled) {
    const jsonPayload = JSON.stringify(meta);
    const payloadSize = formatBytes(jsonPayload.length);
    const memSize = formatBytes(roughSizeOfObject(meta));
    console.log(
      `Done in ${durationMs}ms (${propsCount} props) payload=${payloadSize} mem=${memSize} setup=${setupMs}ms setup ${heapBeforeSetup}->${heapAfterSetup} query ${heapBeforeQuery}->${heapAfterQuery}`,
    );
  } else {
    console.log(`Done in ${durationMs}ms (${propsCount} props) setup=${setupMs}ms`);
  }
} finally {
  checker.close();
  maybeGc();
  if (jsAuditEnabled) {
    console.log(`Closed ${formatMemoryUsage()}`);
  } else {
    console.log("Closed");
  }
}
