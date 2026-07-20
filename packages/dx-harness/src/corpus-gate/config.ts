/**
 * Env-driven configuration for the corpus benchmark gate.
 *
 * The gate NEVER hardcodes a corpus location. The external corpus root arrives
 * exclusively via `VERTER_CORPUS_GATE_DIR`; when that variable is unset the
 * resolution is an HONEST EXPLICIT SKIP (never a silent pass, never a failure).
 * A set-but-invalid directory is a configuration defect and throws loudly —
 * a misconfigured gate must not masquerade as a skip.
 *
 * Knobs (all optional, defaults in parentheses):
 *  - VERTER_CORPUS_GATE_DIR              external corpus root (unset ⇒ skip)
 *  - VERTER_CORPUS_GATE_LABEL            anonymous corpus label ("Corpus A")
 *  - VERTER_CORPUS_GATE_ROUTES           csv of routes (tsserver,tsgo,shared-tsgo)
 *  - VERTER_CORPUS_GATE_SAMPLE           sampled SFC count (40)
 *  - VERTER_CORPUS_GATE_MAX_PROBES_PER_FILE  authored probes per file (24)
 *  - VERTER_CORPUS_GATE_REQUEST_TIMEOUT_MS   per-request timeout (15000)
 *  - VERTER_CORPUS_GATE_LIVENESS_TIMEOUT_MS  wedge liveness-check timeout (10000)
 *  - VERTER_CORPUS_GATE_ROUTE_BUDGET_MS      hard per-route wall-clock cap (1200000)
 *  - VERTER_CORPUS_GATE_READY_CAP_MS         bounded readiness wait (300000)
 *  - VERTER_CORPUS_GATE_SETTLE_CAP_MS        bounded post-ready settle (120000)
 *  - VERTER_CORPUS_GATE_OPEN_SETTLE_MS       bounded per-file open settle (15000)
 *  - VERTER_CORPUS_GATE_RSS_SAMPLE_MS        RSS poll interval (2000)
 *  - VERTER_CORPUS_GATE_RECEIPT              receipt file/dir (temp file)
 *  - VERTER_CORPUS_GATE_BASELINE             prior receipt to diff against
 *  - VERTER_CORPUS_GATE_FILE_DETAIL          "1" ⇒ embed sampled relative paths
 *  - VERTER_CORPUS_GATE_HOVER_P95_MS         hover p95 bar (300)
 *  - VERTER_CORPUS_GATE_DEFINITION_P95_MS    definition p95 bar (500)
 *  - VERTER_CORPUS_GATE_COMPLETION_P95_MS    completion p95 bar (500)
 *  - VERTER_CORPUS_GATE_REFERENCES_P95_MS    references p95 bar (800)
 *  - VERTER_CORPUS_GATE_RSS_MAX_BYTES        per-process RSS ceiling (4 GiB)
 *  - VERTER_CORPUS_GATE_ALLOWED_EMPTY        csv of categories allowed to be
 *                                            empty ("classToken")
 */
import { existsSync, statSync } from "node:fs";
import path from "node:path";

import {
  CORPUS_GATE_ROUTES,
  type CorpusGateConfig,
  type CorpusGateEnvResolution,
  type CorpusGateRoute,
  type CorpusGateThresholds,
} from "./types.js";

export const CORPUS_GATE_DIR_ENV = "VERTER_CORPUS_GATE_DIR";

function positiveInt(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
  const raw = env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || !Number.isInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer, got ${JSON.stringify(raw)}`);
  }
  return value;
}

function parseRoutes(raw: string | undefined): readonly CorpusGateRoute[] {
  if (raw === undefined || raw.trim() === "") return CORPUS_GATE_ROUTES;
  const parsed = raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  if (parsed.length === 0) {
    throw new Error(`VERTER_CORPUS_GATE_ROUTES parsed to zero routes: ${JSON.stringify(raw)}`);
  }
  const seen = new Set<string>();
  const routes: CorpusGateRoute[] = [];
  for (const entry of parsed) {
    if (!(CORPUS_GATE_ROUTES as readonly string[]).includes(entry)) {
      throw new Error(
        `VERTER_CORPUS_GATE_ROUTES contains unknown route ${JSON.stringify(entry)} ` +
          `(valid: ${CORPUS_GATE_ROUTES.join(", ")})`,
      );
    }
    if (!seen.has(entry)) {
      seen.add(entry);
      routes.push(entry as CorpusGateRoute);
    }
  }
  return routes;
}

function parseAllowedEmpty(raw: string | undefined): readonly string[] {
  if (raw === undefined) return ["classToken"];
  return raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

/** Resolve thresholds from env (exported for the hermetic unit suite). */
export function resolveThresholds(env: NodeJS.ProcessEnv): CorpusGateThresholds {
  return {
    hoverP95Ms: positiveInt(env, "VERTER_CORPUS_GATE_HOVER_P95_MS", 300),
    definitionP95Ms: positiveInt(env, "VERTER_CORPUS_GATE_DEFINITION_P95_MS", 500),
    completionP95Ms: positiveInt(env, "VERTER_CORPUS_GATE_COMPLETION_P95_MS", 500),
    referencesP95Ms: positiveInt(env, "VERTER_CORPUS_GATE_REFERENCES_P95_MS", 800),
    rssMaxBytes: positiveInt(env, "VERTER_CORPUS_GATE_RSS_MAX_BYTES", 4 * 1024 * 1024 * 1024),
    allowedEmptyCategories: parseAllowedEmpty(env.VERTER_CORPUS_GATE_ALLOWED_EMPTY),
  };
}

/**
 * Resolve the gate's env into either a runnable config or an honest skip.
 * Unset corpus dir ⇒ `skip` (with the exact variable name in the reason).
 * Set-but-invalid corpus dir ⇒ throws (misconfiguration must be loud).
 */
export function resolveCorpusGateEnv(env: NodeJS.ProcessEnv): CorpusGateEnvResolution {
  const rawDir = env[CORPUS_GATE_DIR_ENV];
  if (rawDir === undefined || rawDir.trim() === "") {
    return {
      kind: "skip",
      reason:
        `${CORPUS_GATE_DIR_ENV} is unset — the corpus gate needs an external corpus root. ` +
        `Export ${CORPUS_GATE_DIR_ENV}=<corpus root> to run; this skip is explicit, not a pass.`,
    };
  }
  const corpusDir = path.resolve(rawDir);
  if (!existsSync(corpusDir) || !statSync(corpusDir).isDirectory()) {
    throw new Error(
      `${CORPUS_GATE_DIR_ENV} is set but is not a directory: ${corpusDir} ` +
        `(a configured gate must not silently skip)`,
    );
  }

  const receiptRaw = env.VERTER_CORPUS_GATE_RECEIPT;
  const baselineRaw = env.VERTER_CORPUS_GATE_BASELINE;
  const config: CorpusGateConfig = {
    corpusDir,
    corpusLabel: env.VERTER_CORPUS_GATE_LABEL?.trim() || "Corpus A",
    routes: parseRoutes(env.VERTER_CORPUS_GATE_ROUTES),
    sampleSize: positiveInt(env, "VERTER_CORPUS_GATE_SAMPLE", 40),
    maxProbesPerFile: positiveInt(env, "VERTER_CORPUS_GATE_MAX_PROBES_PER_FILE", 24),
    requestTimeoutMs: positiveInt(env, "VERTER_CORPUS_GATE_REQUEST_TIMEOUT_MS", 15_000),
    wedgeLivenessTimeoutMs: positiveInt(env, "VERTER_CORPUS_GATE_LIVENESS_TIMEOUT_MS", 10_000),
    routeBudgetMs: positiveInt(env, "VERTER_CORPUS_GATE_ROUTE_BUDGET_MS", 20 * 60_000),
    startupReadyCapMs: positiveInt(env, "VERTER_CORPUS_GATE_READY_CAP_MS", 300_000),
    startupSettleCapMs: positiveInt(env, "VERTER_CORPUS_GATE_SETTLE_CAP_MS", 120_000),
    openSettleCapMs: positiveInt(env, "VERTER_CORPUS_GATE_OPEN_SETTLE_MS", 15_000),
    rssSampleIntervalMs: positiveInt(env, "VERTER_CORPUS_GATE_RSS_SAMPLE_MS", 2_000),
    receiptPath: receiptRaw && receiptRaw.trim() !== "" ? path.resolve(receiptRaw) : null,
    baselinePath: baselineRaw && baselineRaw.trim() !== "" ? path.resolve(baselineRaw) : null,
    includeFileDetail: env.VERTER_CORPUS_GATE_FILE_DETAIL === "1",
    thresholds: resolveThresholds(env),
  };
  return { kind: "run", config };
}
