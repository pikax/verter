/**
 * Shared types for the NATIVE TypeScript reference lane.
 *
 * The lane drives the SAME provider binaries Verter's LSP spawns — tsgo
 * (`<tsgo> --lsp --stdio`) and tsserver (`node tsserver.js
 * --useSyntaxServer=false --disableAutomaticTypingAcquisition`, no plugin) —
 * DIRECTLY against plain `.ts`/`.tsx` files of an external corpus, with no
 * Verter process in the loop. It produces the reference cost of hover /
 * definition / completion / references that the `.vue` path is measured
 * against. The corpus root arrives exclusively via `VERTER_CORPUS_GATE_DIR`;
 * no corpus path or file name is ever committed, and receipts identify files
 * by sample index plus the sample manifest hash.
 */
import type { CorpusKindSummary, CorpusRequestKind } from "../corpus-gate/types.js";

/** The two provider engines the reference lane can drive. */
export type NativeReferenceEngine = "tsgo" | "tsserver";

export const NATIVE_REFERENCE_ENGINES: readonly NativeReferenceEngine[] = ["tsgo", "tsserver"];

/** Resolved lane configuration (see `config.ts` for the env surface). */
export interface NativeReferenceConfig {
  readonly corpusDir: string;
  readonly corpusLabel: string;
  readonly engines: readonly NativeReferenceEngine[];
  readonly sampleSize: number;
  readonly maxProbesPerFile: number;
  readonly requestTimeoutMs: number;
  /** Bound on the single uncounted first-probe warmup (project load). */
  readonly warmupTimeoutMs: number;
  readonly receiptPath: string | null;
  /** Directory receiving the per-engine full-request-trace JSONL files. */
  readonly traceDir: string | null;
  readonly includeFileDetail: boolean;
  /** Explicit tsgo binary override (else VERTER_TSGO_BIN, else repo resolver). */
  readonly tsgoBin: string | null;
  /** Explicit tsserver lib dir (else workspace walk-up, else repo plugin tsdk). */
  readonly tsdk: string | null;
  /**
   * The temp directory receiving the mirror workspace + derived analogues.
   * NEVER inside the repository; the generated files are throwaway private
   * data — the GENERATOR is the deliverable, the generated corpus is not.
   */
  readonly mirrorDir: string;
}

export type NativeReferenceEnvResolution =
  | { readonly kind: "skip"; readonly reason: string }
  | { readonly kind: "run"; readonly config: NativeReferenceConfig };

/** Exact accounting mirror of the corpus gate's per-route accounting. */
export interface NativeAccounting {
  requestsSent: number;
  requestsAnswered: number;
  requestsEmpty: number;
  requestsTimedOut: number;
  requestsErrored: number;
  filesOpened: number;
  filesSkipped: number;
  probesMined: number;
}

/** Startup evidence for one engine session. */
export interface NativeEngineStartup {
  readonly spawnToInitializeMs: number;
  readonly serverName: string | null;
  readonly serverVersion: string | null;
  /**
   * The single uncounted first-probe warmup on the first sampled file. The
   * corpus gate absorbs the provider's project-load cost in its bounded
   * startup settle before any probe is counted; this is the native lane's
   * equivalent, surfaced explicitly instead of hidden in the percentiles.
   */
  readonly warmup: { readonly ms: number; readonly verdict: string } | null;
}

/** One engine session's full report. */
export interface NativeEngineReport {
  readonly engine: NativeReferenceEngine;
  readonly fatalError: string | null;
  /** Where the provider binary came from (never a corpus path). */
  readonly provenance: string;
  readonly startup: NativeEngineStartup;
  readonly accounting: NativeAccounting;
  readonly kinds: Readonly<Record<CorpusRequestKind, CorpusKindSummary>>;
  /** Per sampled file: the first COUNTED request's latency after didOpen. */
  readonly perFileFirstRequestMs: readonly number[];
  /**
   * Server-initiated message tallies by method/event NAME only (names are
   * TS-internal vocabulary, never corpus-identifying).
   */
  readonly serverMessageTallies: Readonly<Record<string, number>>;
  /** Client→server traffic totals for the whole session. */
  readonly clientRequestCount: number;
  readonly clientNotificationCount: number;
  /** JSON text lengths — an ASCII-close approximation of wire bytes. */
  readonly bytesSentApprox: number;
  readonly bytesReceivedApprox: number;
  readonly wallClockMs: number;
}

/** Derivation evidence: what the generator did and what it could not do. */
export interface NativeDerivationReport {
  /** Manifest hash of the SAMPLED `.vue` set (corpus-gate sampler, same
   * pure function — equal to the corpus gate's hash when defaults match). */
  readonly vueSampleManifestHash: string;
  /** Manifest hash of the derived analogue set actually probed. */
  readonly derivedSampleManifestHash: string;
  readonly sampledVueCount: number;
  readonly derivedCount: number;
  readonly skipped: { readonly noScript: number; readonly nonTsScript: number };
  /** Every macro lowering / declared limitation, tallied. */
  readonly tallies: Readonly<Record<string, number>>;
  readonly mirror: {
    readonly copiedFiles: number;
    readonly junctionedNodeModules: number;
    readonly skippedLargeFiles: number;
  };
}

/** The whole-lane receipt. */
export interface NativeReferenceReceipt {
  readonly schemaVersion: 2;
  readonly generatedAt: string;
  readonly corpusLabel: string;
  readonly platform: string;
  readonly nodeVersion: string;
  readonly derivation: NativeDerivationReport;
  readonly sampleSize: number;
  readonly maxProbesPerFile: number;
  readonly requestTimeoutMs: number;
  readonly engines: Partial<Record<NativeReferenceEngine, NativeEngineReport>>;
  /** Relative paths, present only under VERTER_NATIVE_REF_FILE_DETAIL=1. */
  readonly files?: readonly string[];
}
