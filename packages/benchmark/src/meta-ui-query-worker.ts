import { createRequire } from "node:module";

import {
  computeMemoryAuditMeasure,
  ensureMemoryAuditCapable,
  extractAuditTimings,
  type AuditPhaseTimings,
  type MemoryAuditBinding,
  type MemoryAuditCapable,
  type MemoryAuditQueryMeasure,
  type MemoryAuditSiteRow,
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
  /** Opt-in per-component profiling (--profile-audit). */
  profileAudit?: boolean;
}

interface MeasuredQueryResult {
  artifact: NormalizedMetaArtifact;
  latencyMs: number;
  outcome: MetaUiOutcomeBucket;
  /** Worker-process RSS right after the query (bytes). */
  rssBytes: number;
  /** Per-query memory + timing measure; present only in profile-audit mode. */
  memoryAudit?: MemoryAuditQueryMeasure;
}

type ParentMessage =
  | { type: "init"; payload: WorkerInitPayload }
  | { type: "query"; requestId: number; component: PreparedComponentSnapshot }
  | { type: "sites"; requestId: number; topK: number };

const require = createRequire(import.meta.url);

let checkerPromise: Promise<any> | null = null;
/** Enabled runtime-audit handle; null when profile audit is off. */
let memoryAudit: MemoryAuditCapable | null = null;
/** Profile mode on a verter checker: phase timings are collectable. */
let profileAuditVerter = false;
/**
 * Audited native query for phase timings; resolved LAZILY at query time
 * (memoized) because the compat checker materialises its runtime
 * session on first file activity, not at construction.
 */
let auditedNativeQuery: ((absolutePath: string) => Uint8Array | null) | null = null;

process.on("message", (message: ParentMessage) => {
  void handleMessage(message);
});

async function handleMessage(message: ParentMessage): Promise<void> {
  if (message.type === "init") {
    try {
      if (message.payload.profileAudit) {
        // Loud-failure setup gate + runtime enable handshake: a binding
        // that predates the runtime memory-audit surface throws here,
        // which surfaces as a fatal message and fails the runner at
        // setup. Sampling arming rides VERTER_MEMORY_AUDIT_SAMPLE
        // (inherited env, read once by the native side).
        memoryAudit = ensureMemoryAuditCapable(require("@verter/native") as MemoryAuditBinding);
      }
      const checker = await createChecker(
        message.payload.uiRoot,
        message.payload.checkerConfig,
        message.payload.backend,
        message.payload.profileAudit === true,
      );
      profileAuditVerter =
        message.payload.profileAudit === true && message.payload.backend === "verter";
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
    return;
  }

  if (message.type === "sites") {
    // End-of-pass sampled allocation-site collection. Additive and
    // never a loud failure: null covers memory audit off, an older
    // instrumented binary without the export, and sampling not armed
    // via VERTER_MEMORY_AUDIT_SAMPLE (the loud-failure contract stays
    // owned by the snapshot gate at init).
    let sites: MemoryAuditSiteRow[] | null = null;
    try {
      sites = memoryAudit ? memoryAudit.sites(message.topK) : null;
    } catch (error) {
      console.error(
        `meta-ui worker: memoryAuditSites collection failed: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
    process.send?.({ type: "sites", requestId: message.requestId, sites });
  }
}

async function createChecker(
  uiRoot: string,
  checkerConfig: Record<string, unknown>,
  backend: MetaUiBackend,
  profileAudit: boolean,
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
    // Profile mode: have the native runtime capture per-request audit
    // records so the audited query variant can report phase timings.
    ...(profileAudit ? { logging: { audit: true } } : {}),
  });
}

/**
 * Locate the audited native query on a verter compat checker. Reaches
 * through the checker's runtime session to the native `MetaSession`
 * (`getComponentMetaWithAudit`) — additive and defensively guarded:
 * `null` (no timings) whenever the internals do not match, never a
 * failure.
 */
function resolveAuditedNativeQuery(
  checker: any,
): ((absolutePath: string) => Uint8Array | null) | null {
  const nativeSession = checker?._session?._nativeSession;
  if (!nativeSession || typeof nativeSession.getComponentMetaWithAudit !== "function") {
    return null;
  }
  return (absolutePath: string) => nativeSession.getComponentMetaWithAudit(absolutePath);
}

/**
 * Run the audited native query and extract its phase timings. The
 * audited call performs the same semantic work as the plain query (one
 * shared engine) plus audit capture; the follow-up compat query reads
 * warm state for the artifact. Returns undefined on any failure —
 * timings are additive.
 */
function measureAuditedQuery(absolutePath: string): AuditPhaseTimings | undefined {
  const query = auditedNativeQuery;
  if (!query) {
    return undefined;
  }
  try {
    const bundleBytes = query(absolutePath);
    if (!bundleBytes) {
      return undefined;
    }
    const bundle = JSON.parse(Buffer.from(bundleBytes).toString("utf8")) as {
      record?: unknown;
    };
    return extractAuditTimings(bundle.record) ?? undefined;
  } catch (error) {
    console.error(
      `meta-ui worker: audited query failed for ${absolutePath}: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
    return undefined;
  }
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
  // Profile mode (verter): the MEASURED query is the audited native
  // variant — same shared engine, plus audit capture — so the phase
  // timings describe the query that was actually timed. The compat
  // query that produces the artifact then reads warm state (identical
  // result by cache correctness) outside the measured window.
  if (profileAuditVerter && !auditedNativeQuery) {
    auditedNativeQuery = resolveAuditedNativeQuery(checker);
  }
  let timings: AuditPhaseTimings | undefined;
  let latencyMs: number;
  let raw: unknown;
  if (auditedNativeQuery) {
    const startedAt = performance.now();
    timings = measureAuditedQuery(component.absolutePath);
    latencyMs = performance.now() - startedAt;
    raw = await checker.getComponentMeta(component.absolutePath);
  } else {
    const startedAt = performance.now();
    raw = await checker.getComponentMeta(component.absolutePath);
    latencyMs = performance.now() - startedAt;
  }
  const artifact = normalizeComponentMetaArtifact(component.relativePath, raw);
  const outcome: MetaUiOutcomeBucket = artifact.diagnostics.length > 0 ? "degraded" : "success";
  const result: MeasuredQueryResult = {
    artifact,
    latencyMs,
    outcome,
    rssBytes: process.memoryUsage().rss,
  };
  if (audit && auditBefore) {
    const usage = process.memoryUsage();
    result.memoryAudit = {
      ...computeMemoryAuditMeasure(auditBefore, audit.snapshot(), {
        rss: usage.rss,
        heapUsed: usage.heapUsed,
      }),
      ...(timings ? { timings } : {}),
    };
  }
  return result;
}
