import { createRequire } from "node:module";

import {
  computeMemoryAuditMeasure,
  ensureMemoryAuditCapable,
  type MemoryAuditBinding,
  type MemoryAuditCapable,
  type MemoryAuditQueryMeasure,
  type MetaUiBackend,
  type MetaUiOutcomeBucket,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";
import { normalizeComponentMetaArtifact } from "./component-meta-artifact.js";
import { loadVerterCompatModule } from "./verter-compat.js";

interface PreparedComponentSnapshot {
  absolutePath: string;
  relativePath: string;
  transformedSource: string;
}

interface WorkerInitPayload {
  backend: MetaUiBackend;
  uiRoot: string;
  checkerConfig: Record<string, unknown>;
  components: PreparedComponentSnapshot[];
  /** Opt-in deep memory audit (--memory-audit). */
  memoryAudit?: boolean;
}

interface MeasuredQueryResult {
  artifact: NormalizedMetaArtifact;
  latencyMs: number;
  outcome: MetaUiOutcomeBucket;
  /** Worker-process RSS right after the query (bytes). */
  rssBytes: number;
  /** Per-query memory measure; present only in memory-audit mode. */
  memoryAudit?: MemoryAuditQueryMeasure;
}

type ParentMessage =
  | { type: "init"; payload: WorkerInitPayload }
  | { type: "query"; requestId: number; component: PreparedComponentSnapshot };

const require = createRequire(import.meta.url);

let checkerPromise: Promise<any> | null = null;
/** Validated instrumented-binding handle; null when memory audit is off. */
let memoryAudit: MemoryAuditCapable | null = null;

process.on("message", (message: ParentMessage) => {
  void handleMessage(message);
});

async function handleMessage(message: ParentMessage): Promise<void> {
  if (message.type === "init") {
    try {
      if (message.payload.memoryAudit) {
        // Loud-failure setup gate: a non-instrumented @verter/native
        // (missing exports or a null snapshot) throws here, which
        // surfaces as a fatal message and fails the runner at setup.
        memoryAudit = ensureMemoryAuditCapable(require("@verter/native") as MemoryAuditBinding);
      }
      const checker = await createChecker(
        message.payload.uiRoot,
        message.payload.checkerConfig,
        message.payload.backend,
      );
      for (const component of message.payload.components) {
        checker.updateFile(component.absolutePath, component.transformedSource);
      }
      checkerPromise = Promise.resolve(checker);
      process.send?.({ type: "ready" });
    } catch (error) {
      process.send?.({
        type: "fatal",
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
      });
    }
    return;
  }

  if (message.type === "query") {
    try {
      if (!checkerPromise) {
        throw new Error("meta-ui benchmark worker was queried before initialization");
      }
      const checker = await checkerPromise;
      const result = await executeMeasuredQuery(checker, message.component);
      process.send?.({ type: "result", requestId: message.requestId, result });
    } catch (error) {
      process.send?.({
        type: "error",
        requestId: message.requestId,
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
      });
    }
  }
}

async function createChecker(
  uiRoot: string,
  checkerConfig: Record<string, unknown>,
  backend: MetaUiBackend,
): Promise<any> {
  if (backend === "vue-component-meta") {
    const module = require("vue-component-meta");
    if (typeof module.createCheckerByJson === "function") {
      return module.createCheckerByJson(uiRoot, checkerConfig, {
        forceUseTs: true,
        schema: true,
      });
    }
    throw new Error("Installed vue-component-meta does not expose createCheckerByJson().");
  }

  const { createCheckerByJson } = await loadVerterCompatModule();
  return createCheckerByJson(uiRoot, checkerConfig, {
    forceUseTs: true,
    schema: { literalBooleanSchema: true },
    runtimeMode: "dedicated",
  });
}

async function executeMeasuredQuery(
  checker: any,
  component: PreparedComponentSnapshot,
): Promise<MeasuredQueryResult> {
  // Memory-audit capture brackets the query OUTSIDE the timed window:
  // re-arm the allocator high-water mark and snapshot the counters
  // before the query, then fold the post-query delta into the result.
  const audit = memoryAudit;
  let auditBefore = null;
  if (audit) {
    audit.resetHighWater();
    auditBefore = audit.snapshot();
  }
  const startedAt = performance.now();
  const raw = await checker.getComponentMeta(component.absolutePath);
  const artifact = normalizeComponentMetaArtifact(component.relativePath, raw);
  const latencyMs = performance.now() - startedAt;
  const outcome: MetaUiOutcomeBucket = artifact.diagnostics.length > 0 ? "degraded" : "success";
  const result: MeasuredQueryResult = {
    artifact,
    latencyMs,
    outcome,
    rssBytes: process.memoryUsage().rss,
  };
  if (audit && auditBefore) {
    const usage = process.memoryUsage();
    result.memoryAudit = computeMemoryAuditMeasure(auditBefore, audit.snapshot(), {
      rss: usage.rss,
      heapUsed: usage.heapUsed,
    });
  }
  return result;
}
