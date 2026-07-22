/**
 * Env-driven configuration for the native TypeScript reference lane.
 *
 * Mirrors the corpus gate's contract: the corpus root arrives EXCLUSIVELY via
 * `VERTER_CORPUS_GATE_DIR` (unset ⇒ honest explicit skip; set-but-invalid ⇒
 * loud throw). Every other knob is optional with a deterministic default.
 *
 * Knobs (defaults in parentheses):
 *  - VERTER_CORPUS_GATE_DIR                  external corpus root (unset ⇒ skip)
 *  - VERTER_NATIVE_REF_LABEL                 anonymous corpus label ("Corpus A")
 *  - VERTER_NATIVE_REF_ENGINES               csv of engines (tsgo,tsserver)
 *  - VERTER_NATIVE_REF_SAMPLE                sampled .ts/.tsx count (40)
 *  - VERTER_NATIVE_REF_MAX_PROBES_PER_FILE   authored probes per file (24)
 *  - VERTER_NATIVE_REF_REQUEST_TIMEOUT_MS    per-request timeout (15000)
 *  - VERTER_NATIVE_REF_WARMUP_TIMEOUT_MS     first-probe warmup bound (120000)
 *  - VERTER_NATIVE_REF_RECEIPT               receipt file path (temp file)
 *  - VERTER_NATIVE_REF_TRACE_DIR             trace JSONL dir (receipt's dir)
 *  - VERTER_NATIVE_REF_FILE_DETAIL           "1" ⇒ embed sampled relative paths
 *  - VERTER_NATIVE_REF_TSGO_BIN              explicit tsgo binary override
 *  - VERTER_NATIVE_REF_TSDK                  explicit tsserver lib dir override
 *  - VERTER_NATIVE_REF_MIRROR_DIR            mirror-workspace temp dir override
 */
import { existsSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { CORPUS_GATE_DIR_ENV } from "../corpus-gate/config.js";
import {
  NATIVE_REFERENCE_ENGINES,
  type NativeReferenceConfig,
  type NativeReferenceEngine,
  type NativeReferenceEnvResolution,
} from "./types.js";

function positiveInt(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
  const raw = env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || !Number.isInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer, got ${JSON.stringify(raw)}`);
  }
  return value;
}

function parseEngines(raw: string | undefined): readonly NativeReferenceEngine[] {
  if (raw === undefined || raw.trim() === "") return NATIVE_REFERENCE_ENGINES;
  const parsed = raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  if (parsed.length === 0) {
    throw new Error(`VERTER_NATIVE_REF_ENGINES parsed to zero engines: ${JSON.stringify(raw)}`);
  }
  const engines: NativeReferenceEngine[] = [];
  for (const entry of parsed) {
    if (!(NATIVE_REFERENCE_ENGINES as readonly string[]).includes(entry)) {
      throw new Error(
        `VERTER_NATIVE_REF_ENGINES contains unknown engine ${JSON.stringify(entry)} ` +
          `(valid: ${NATIVE_REFERENCE_ENGINES.join(", ")})`,
      );
    }
    if (!engines.includes(entry as NativeReferenceEngine)) {
      engines.push(entry as NativeReferenceEngine);
    }
  }
  return engines;
}

/** Resolve the lane's env into either a runnable config or an honest skip. */
export function resolveNativeReferenceEnv(env: NodeJS.ProcessEnv): NativeReferenceEnvResolution {
  const rawDir = env[CORPUS_GATE_DIR_ENV];
  if (rawDir === undefined || rawDir.trim() === "") {
    return {
      kind: "skip",
      reason:
        `${CORPUS_GATE_DIR_ENV} is unset — the native reference lane needs an external corpus ` +
        `root. Export ${CORPUS_GATE_DIR_ENV}=<corpus root> to run; this skip is explicit.`,
    };
  }
  const corpusDir = path.resolve(rawDir);
  if (!existsSync(corpusDir) || !statSync(corpusDir).isDirectory()) {
    throw new Error(
      `${CORPUS_GATE_DIR_ENV} is set but is not a directory: ${corpusDir} ` +
        `(a configured lane must not silently skip)`,
    );
  }

  const receiptRaw = env.VERTER_NATIVE_REF_RECEIPT;
  const receiptPath =
    receiptRaw && receiptRaw.trim() !== ""
      ? path.resolve(receiptRaw)
      : path.join(tmpdir(), `verter-native-reference-${process.pid}.json`);
  const traceRaw = env.VERTER_NATIVE_REF_TRACE_DIR;
  const config: NativeReferenceConfig = {
    corpusDir,
    corpusLabel: env.VERTER_NATIVE_REF_LABEL?.trim() || "Corpus A",
    engines: parseEngines(env.VERTER_NATIVE_REF_ENGINES),
    sampleSize: positiveInt(env, "VERTER_NATIVE_REF_SAMPLE", 40),
    maxProbesPerFile: positiveInt(env, "VERTER_NATIVE_REF_MAX_PROBES_PER_FILE", 24),
    requestTimeoutMs: positiveInt(env, "VERTER_NATIVE_REF_REQUEST_TIMEOUT_MS", 15_000),
    warmupTimeoutMs: positiveInt(env, "VERTER_NATIVE_REF_WARMUP_TIMEOUT_MS", 120_000),
    receiptPath,
    traceDir:
      traceRaw && traceRaw.trim() !== "" ? path.resolve(traceRaw) : path.dirname(receiptPath),
    includeFileDetail: env.VERTER_NATIVE_REF_FILE_DETAIL === "1",
    tsgoBin: env.VERTER_NATIVE_REF_TSGO_BIN?.trim() || null,
    tsdk: env.VERTER_NATIVE_REF_TSDK?.trim() || null,
    mirrorDir:
      env.VERTER_NATIVE_REF_MIRROR_DIR && env.VERTER_NATIVE_REF_MIRROR_DIR.trim() !== ""
        ? path.resolve(env.VERTER_NATIVE_REF_MIRROR_DIR)
        : path.join(tmpdir(), `verter-native-mirror-${process.pid}`),
  };
  return { kind: "run", config };
}
