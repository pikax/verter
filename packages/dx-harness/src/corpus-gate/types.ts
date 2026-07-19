/**
 * Shared types for the corpus benchmark gate.
 *
 * The corpus gate drives the REAL `verter-lsp` binary over stdio against an
 * EXTERNAL (never committed) corpus of Vue SFCs on every type-provider route,
 * fires authored-position requests, and emits a machine-readable receipt so
 * runs are comparable over time. The corpus location arrives exclusively via
 * `VERTER_CORPUS_GATE_DIR`; no corpus path or file name is ever committed, and
 * receipts identify the corpus only by an anonymous label (default "Corpus A").
 */
import type { EditorNeutralProviderRoute } from "@verter/lsp-test-client";

/** The three verter-lsp type-provider routes the gate exercises. */
export type CorpusGateRoute = EditorNeutralProviderRoute;

export const CORPUS_GATE_ROUTES: readonly CorpusGateRoute[] = ["tsserver", "tsgo", "shared-tsgo"];

/** The authored-position request kinds the gate fires. */
export type CorpusRequestKind = "hover" | "definition" | "completion" | "references";

export const CORPUS_REQUEST_KINDS: readonly CorpusRequestKind[] = [
  "hover",
  "definition",
  "completion",
  "references",
];

/**
 * How one fired request settled.
 *  - `ok`      — a non-empty result arrived in time.
 *  - `empty`   — a result arrived but carried no content (null / no items /
 *                no locations / blank hover).
 *  - `timeout` — the client-side per-request timeout fired.
 *  - `error`   — the server returned an error response or the transport threw.
 */
export type CorpusRequestVerdict = "ok" | "empty" | "timeout" | "error";

/** One fired authored-position request and how it settled. */
export interface CorpusRequestObservation {
  readonly kind: CorpusRequestKind;
  /** Probe category (componentTag, propBind, interp, importName, …). */
  readonly category: string;
  readonly ms: number;
  readonly verdict: CorpusRequestVerdict;
  /** `empty` verdict on a category the config does not allow to be empty. */
  readonly unexpectedEmpty: boolean;
}

/** Latency + outcome summary for one request kind on one route. */
export interface CorpusKindSummary {
  readonly count: number;
  readonly p50Ms: number;
  readonly p90Ms: number;
  readonly p95Ms: number;
  readonly maxMs: number;
  /** Requests slower than the interactive threshold (2500ms). */
  readonly over2500Count: number;
  readonly timeoutCount: number;
  readonly emptyCount: number;
  readonly unexpectedEmptyCount: number;
  readonly errorCount: number;
}

/** RSS trend for one tracked process over the session. */
export interface CorpusProcessMemoryTrend {
  /** Structural role, e.g. "verter-lsp" | "provider" | "relay". */
  readonly label: string;
  readonly pid: number | null;
  /** False ⇒ the platform could not read RSS; assertion is skipped explicitly. */
  readonly supported: boolean;
  readonly sampleCount: number;
  readonly firstRssBytes: number | null;
  readonly lastRssBytes: number | null;
  readonly maxRssBytes: number | null;
  /** Downsampled series (bounded length) for trend inspection. */
  readonly samples: readonly { readonly atMs: number; readonly rssBytes: number }[];
}

/** Exact request/response accounting for one route (non-vacuity evidence). */
export interface CorpusRouteAccounting {
  readonly requestsSent: number;
  readonly requestsAnswered: number;
  readonly requestsEmpty: number;
  readonly requestsTimedOut: number;
  readonly requestsErrored: number;
  /** Sent requests whose promise NEVER settled (hard-race fired): wedge class. */
  readonly requestsAbandoned: number;
  readonly filesOpened: number;
  readonly filesSkipped: number;
  readonly probesMined: number;
}

/** Startup evidence for one route session (bounded, best-effort — never hangs). */
export interface CorpusRouteStartup {
  readonly initializeMs: number;
  readonly readyObserved: boolean;
  readonly syncObserved: boolean;
  readonly quiesced: boolean;
  readonly settleMs: number;
}

/** Session-liveness evidence (`$/verter/getStatistics` checks). */
export interface CorpusRouteLiveness {
  readonly checks: number;
  readonly failures: number;
}

/** Wall-clock budget accounting for one route. */
export interface CorpusRouteWallClock {
  readonly budgetMs: number;
  readonly elapsedMs: number;
  readonly budgetExceeded: boolean;
}

/** The per-route section of the receipt. */
export interface CorpusRouteReport {
  readonly route: CorpusGateRoute;
  /** True when the route session ran to the end of its planned work. */
  readonly completed: boolean;
  /** Hard wedge: a request never settled or the liveness check went dark. */
  readonly wedged: boolean;
  readonly wedgeDetail: string | null;
  /** Non-wedge fatal error (spawn/initialize failure), if any. */
  readonly fatalError: string | null;
  readonly startup: CorpusRouteStartup;
  readonly accounting: CorpusRouteAccounting;
  readonly kinds: Readonly<Record<CorpusRequestKind, CorpusKindSummary>>;
  readonly memory: readonly CorpusProcessMemoryTrend[];
  readonly liveness: CorpusRouteLiveness;
  readonly wallClock: CorpusRouteWallClock;
  /**
   * Stable hash of the sampled relative-path list — proves two receipts ran
   * the same sample without embedding corpus file names.
   */
  readonly sampleManifestHash: string;
  /** Relative file paths — included only when the config opts in. */
  readonly files?: readonly string[];
}

/** Acceptance-bar thresholds (all env-overridable). */
export interface CorpusGateThresholds {
  readonly hoverP95Ms: number;
  readonly definitionP95Ms: number;
  readonly completionP95Ms: number;
  readonly referencesP95Ms: number;
  /** Per tracked process RSS ceiling (bytes). */
  readonly rssMaxBytes: number;
  /** Probe categories whose `empty` results are expected (not failures). */
  readonly allowedEmptyCategories: readonly string[];
}

/** Fully resolved gate configuration (corpus path never enters the receipt). */
export interface CorpusGateConfig {
  readonly corpusDir: string;
  readonly corpusLabel: string;
  readonly routes: readonly CorpusGateRoute[];
  readonly sampleSize: number;
  readonly maxProbesPerFile: number;
  readonly requestTimeoutMs: number;
  readonly wedgeLivenessTimeoutMs: number;
  readonly routeBudgetMs: number;
  readonly startupReadyCapMs: number;
  readonly startupSettleCapMs: number;
  readonly openSettleCapMs: number;
  readonly rssSampleIntervalMs: number;
  /** Receipt destination (file or directory); null ⇒ temp file. */
  readonly receiptPath: string | null;
  /** Prior receipt to diff against; null ⇒ no compare. */
  readonly baselinePath: string | null;
  /** Include sampled relative file paths in the receipt (default false). */
  readonly includeFileDetail: boolean;
  readonly thresholds: CorpusGateThresholds;
}

/** Env resolution: run with a config, or an honest explicit skip. */
export type CorpusGateEnvResolution =
  | { readonly kind: "run"; readonly config: CorpusGateConfig }
  | { readonly kind: "skip"; readonly reason: string };

/** Receipt-embedded config echo (structural knobs only — never a path). */
export interface CorpusGateConfigEcho {
  readonly routes: readonly CorpusGateRoute[];
  readonly sampleSize: number;
  readonly maxProbesPerFile: number;
  readonly requestTimeoutMs: number;
  readonly wedgeLivenessTimeoutMs: number;
  readonly routeBudgetMs: number;
  readonly thresholds: CorpusGateThresholds;
}

/** The machine-readable receipt one gate run emits. */
export interface CorpusGateReceipt {
  readonly schemaVersion: 1;
  readonly harness: "corpus-gate";
  readonly createdAt: string;
  readonly corpusLabel: string;
  readonly corpus: {
    /** Total `.vue` files discovered under the corpus root. */
    readonly vueFileCount: number;
    readonly sampledCount: number;
  };
  readonly config: CorpusGateConfigEcho;
  readonly routes: Partial<Record<CorpusGateRoute, CorpusRouteReport>>;
  /** Acceptance-bar failures over the whole run; empty ⇒ pass. */
  readonly assertionFailures: readonly string[];
  readonly pass: boolean;
}
