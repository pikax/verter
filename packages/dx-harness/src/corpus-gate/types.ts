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

/**
 * What a sampled process is, structurally.
 *  - `server`     — the spawned `verter-lsp` process itself.
 *  - `provider`   — the type provider the server (or the relay) owns.
 *  - `descendant` — any other process in the server/relay tree. Sampled and
 *                   bounded like the rest: an unsampled descendant is exactly
 *                   how a memory blow-up escapes the per-process ceiling.
 */
export type CorpusProcessRole = "server" | "provider" | "descendant";

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
  /** Structural role; absent on receipts written before roles existed. */
  readonly role?: CorpusProcessRole;
  /** Parent pid as observed in the process table (evidence for the role). */
  readonly parentPid?: number | null;
  /** Process image/command name as observed (evidence, never a matcher input). */
  readonly image?: string | null;
}

/**
 * How the sampled provider process was established.
 *  - `verified`     — the sampled pid exists and is structurally a descendant
 *                     of the server (or of the relay the harness spawned).
 *  - `mismatched`   — the sampled pid is the server itself, the harness itself,
 *                     or something outside the server/relay tree: the sampler
 *                     is bounding the WRONG process.
 *  - `missing`      — no provider pid was ever observed, or it had vanished
 *                     from the process table when sampling started.
 *  - `unobservable` — the platform could not enumerate the process table at
 *                     all; the provider ceiling is explicitly unenforced.
 */
export type CorpusProviderAttributionStatus =
  | "verified"
  | "mismatched"
  | "missing"
  | "unobservable";

/** Provable attribution of the provider RSS sample (gate-integrity evidence). */
export interface CorpusProviderAttribution {
  readonly status: CorpusProviderAttributionStatus;
  readonly providerPid: number | null;
  /** Human-readable evidence for the status — always populated. */
  readonly detail: string;
  /**
   * Server/relay-tree processes that were discovered but never sampled. MUST
   * be empty: an unsampled tree member is unbounded memory.
   */
  readonly unattributedPids: readonly number[];
  /** Distinct processes actually sampled over the session. */
  readonly sampledProcessCount: number;
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

/**
 * How route sessions were scheduled.
 *  - `serial`   — one route session at a time on this executor (the default).
 *  - `parallel` — route sessions run concurrently on this executor.
 *
 * CI fan-out (one route per machine) is NOT `parallel`: each machine runs a
 * single-route `serial` gate, which is why its latency stays gating.
 */
export type CorpusExecutionTopology = "serial" | "parallel";

/**
 * What the operator/CI attests about this executor.
 *  - `dedicated`  — this process owns the machine/container (pinned resources).
 *  - `shared`     — other work runs here; latency is not a valid measurement.
 *  - `unattested` — nothing was claimed (the local-developer default).
 */
export type CorpusExecutorAttestation = "dedicated" | "shared" | "unattested";

/** Whether a route's measurements were taken free of gate-induced contention. */
export type CorpusIsolationMode = "isolated" | "contended";

/**
 * Per-route isolation record — first-class receipt data, recorded by the
 * ORCHESTRATOR from what it observed, never self-declared by a route runner.
 *
 * `latencyGating` is the single authority for whether this route's latency
 * percentiles are allowed to decide pass/fail. Contended routes still report
 * their percentiles, but as clearly labelled ADVISORY numbers.
 */
export interface CorpusRouteIsolation {
  readonly topology: CorpusExecutionTopology;
  readonly executor: CorpusExecutorAttestation;
  readonly mode: CorpusIsolationMode;
  /** Peak concurrent route sessions observed during this route's window. */
  readonly observedConcurrentRoutes: number;
  /** True ⇒ latency percentiles gate. False ⇒ they are advisory only. */
  readonly latencyGating: boolean;
  /** Why the mode was chosen — always populated, never a bare claim. */
  readonly evidence: string;
  /** A `dedicated` attestation refuted by observed concurrency: a hard defect. */
  readonly attestationContradicted: boolean;
}

/** Opt-in early-stop accounting (census detail traded for speed). */
export interface CorpusRouteEarlyStop {
  readonly enabled: boolean;
  readonly stopped: boolean;
  readonly reason: string | null;
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
   * Isolation record for this route. The route runner emits a fail-closed
   * UNPROVEN value; the orchestrator overwrites it with what it observed.
   * Optional in the TYPE only so receipts written before isolation existed
   * still load — a missing value is treated as UNPROVEN (never gating).
   */
  readonly isolation?: CorpusRouteIsolation;
  /** Provider-sample attribution evidence (gate integrity). */
  readonly providerAttribution?: CorpusProviderAttribution;
  /** Opt-in early stop; `enabled: false` on the default path. */
  readonly earlyStop?: CorpusRouteEarlyStop;
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

/** Machine resources observed at run time (capability gate for parallelism). */
export interface CorpusMachineCapability {
  readonly cpuCount: number;
  readonly totalMemBytes: number;
}

/** Fully resolved gate configuration (corpus path never enters the receipt). */
export interface CorpusGateConfig {
  readonly corpusDir: string;
  readonly corpusLabel: string;
  readonly routes: readonly CorpusGateRoute[];
  /** Requested scheduling topology; downgraded when the machine cannot isolate. */
  readonly topology: CorpusExecutionTopology;
  /** What the operator/CI attests about this executor. */
  readonly executor: CorpusExecutorAttestation;
  /** `true` ⇒ a route whose latency is not gating FAILS the run (CI strictness). */
  readonly requireIsolatedLatency: boolean;
  /** Whole-run wall-clock target (reported; fatal only when opted in). */
  readonly gateBudgetMs: number;
  readonly gateBudgetFatal: boolean;
  /** Opt-in: stop a route once its verdict is already decided by a failure. */
  readonly fastMode: boolean;
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

/** How this run was executed — receipt-level, first-class. */
export interface CorpusGateExecution {
  readonly requestedTopology: CorpusExecutionTopology;
  readonly effectiveTopology: CorpusExecutionTopology;
  /** Why a requested topology was not honoured (capability gate), if so. */
  readonly downgradeReason: string | null;
  readonly executor: CorpusExecutorAttestation;
  readonly machine: CorpusMachineCapability;
  readonly requireIsolatedLatency: boolean;
  /**
   * Opt-in early stop. `true` ⇒ routes may stop once a failure already decided
   * their verdict, so per-kind census counts are DELIBERATELY incomplete.
   */
  readonly fastMode: boolean;
  /** Routes whose latency percentiles gated (the rest are advisory). */
  readonly latencyGatingRoutes: readonly CorpusGateRoute[];
  /** Routes whose latency percentiles are ADVISORY (contended measurement). */
  readonly latencyAdvisoryRoutes: readonly CorpusGateRoute[];
}

/** Whole-run wall-clock budget accounting. */
export interface CorpusGateBudget {
  readonly targetMs: number;
  readonly actualMs: number;
  readonly exceeded: boolean;
  /** True ⇒ a breach was promoted to a gating failure (opt-in). */
  readonly fatal: boolean;
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
  /** Execution topology + isolation summary; absent on pre-topology receipts. */
  readonly execution?: CorpusGateExecution;
  /** Whole-run wall-clock budget accounting; absent on pre-budget receipts. */
  readonly budget?: CorpusGateBudget;
  /**
   * Recorded-but-NOT-gating observations, every one prefixed `ADVISORY` — most
   * importantly latency breaches on contended routes. Never merged into
   * `assertionFailures`, never able to flip `pass`.
   */
  readonly advisories?: readonly string[];
}
