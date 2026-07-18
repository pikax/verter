/**
 * Shared types for the endurance/soak harness.
 *
 * The harness drives the REAL `verter-lsp` binary over stdio for long-session
 * IDE workloads and asserts stability: the provider stays alive, every request
 * is answered or properly cancelled (never silently dropped), latency and RSS
 * stay bounded, and features still answer correctly after sustained load.
 */
import type { EditorNeutralProviderRoute } from "@verter/lsp-test-client";

/** The three verter-lsp type-provider routes the harness can exercise. */
export type EnduranceProviderRoute = EditorNeutralProviderRoute;

export const ENDURANCE_PROVIDER_ROUTES: readonly EnduranceProviderRoute[] = [
  "tsserver",
  "tsgo",
  "shared-tsgo",
];

/**
 * How a sent request settled:
 *  - `answered`   — the server returned a result (possibly `null`).
 *  - `cancelled`  — the server returned a proper cancellation error response
 *                   (LSP -32800 RequestCancelled / -32801 ContentModified).
 *  - `errored`    — the server returned any OTHER LSP error response. The
 *                   request was not dropped, but an unexpected error under a
 *                   normal workload is a defect signal and fails the run.
 *  - `unanswered` — no protocol answer ever arrived (client-side timeout, or
 *                   the transport died). This is the "silent drop" the harness
 *                   exists to catch; any occurrence fails the run.
 */
export type RequestClassification = "answered" | "cancelled" | "errored" | "unanswered";

/** Latency percentiles over a sample set (milliseconds). */
export interface PercentileSummary {
  readonly count: number;
  readonly p50: number;
  readonly p95: number;
  readonly p99: number;
  readonly max: number;
  readonly mean: number;
}

/** A percentile summary for one time window of the run. */
export interface WindowSummary extends PercentileSummary {
  readonly windowIndex: number;
  readonly startedAtMs: number;
}

/** Env-tunable knobs for the endurance harness (see `config.ts`). */
export interface EnduranceConfig {
  /** Provider route for this run (VERTER_ENDURANCE_PROVIDER, default tsgo). */
  readonly route: EnduranceProviderRoute;
  /** Per-request timeout for storm/soak traffic (ms). */
  readonly requestTimeoutMs: number;
  /** Per-request timeout for strict (serial, asserted) probe requests (ms). */
  readonly probeTimeoutMs: number;
  /** Hard per-request latency bound for strict probes (ms). */
  readonly probeLatencyBoundMs: number;
  /**
   * Absolute p95 latency bound asserted by the SOAK and scale lanes (ms) —
   * PER-ROUTE by default: 2000 for tsgo/shared-tsgo (measured 96–448ms,
   * 4–20× headroom — a tight regression guard); 5000 for tsserver.
   * Rationale: tsserver is a single-threaded engine whose serial-throughput
   * latency scales with workspace size × concurrency depth × per-request
   * cost (measured 2365–4351ms across lanes on the debug build, ±40%
   * variance). 5000ms is a grounded CATASTROPHIC-degradation ceiling — ~2×
   * observed serial capacity — that catches any regression toward the
   * pre-fix wedge (10s+ / dropped requests) while accommodating normal
   * serial-engine variance. It is NOT loosening-to-green: the DEGRADATION
   * TREND stays the primary, uniformly-asserted latency-over-time gate on
   * all routes, and the D2 stability contract (zero unanswered, alive,
   * correct-after) is unchanged and uniform. Env: VERTER_ENDURANCE_P95_MAX_MS.
   */
  readonly p95MaxMs: number;
  /**
   * Storm p95 latency bound (ms) — PER-ROUTE by default on the same grounded
   * model as {@link p95MaxMs}: 2000 for tsgo/shared-tsgo, 5000 for tsserver
   * (its serial-throughput capacity under full-throttle concurrency). The
   * STABILITY bound (zero unanswered, alive, post-storm correctness) is
   * identical on every route. Env: VERTER_ENDURANCE_STORM_P95_MAX_MS.
   */
  readonly stormP95MaxMs: number;
  /** Late-window p95 must be <= early-window p95 * this factor (soak trend). */
  readonly degradationFactor: number;
  /** Latency time-window size (ms) for trend analysis. */
  readonly windowMs: number;
  /** Max in-flight LSP requests the harness allows itself (semaphore). */
  readonly maxInFlight: number;
  /** RSS ceiling for the verter-lsp process (bytes); 4 GiB default. */
  readonly rssMaxBytes: number;
  /** RSS sampling interval (ms). */
  readonly rssSampleMs: number;
  /** heavy-update loop iteration count. */
  readonly heavyUpdateCycles: number;
  /** Storm sustained duration (ms). */
  readonly stormDurationMs: number;
  /** Storm worker count (each worker keeps <=1 request in flight). */
  readonly stormWorkers: number;
  /** Soak sustained duration (ms). */
  readonly soakDurationMs: number;
  /**
   * Typing cadence in didChange notifications per second (VERTER_ENDURANCE_TYPING_CPS,
   * default 12 — human-realistic sustained typing; the hover/definition storm
   * rate is separate and stays aggressive). Raise toward ~80 to probe the
   * superhuman throughput ceiling (informational only, never asserted).
   */
  readonly typingCps: number;
  /** External corpus dir for the scale lane (read-only), if any. */
  readonly corpusDir: string | null;
  /** Generate a synthetic corpus for the scale lane when no corpus dir is set. */
  readonly syntheticScale: boolean;
  /** Files to open in the scale lane. */
  readonly scaleOpenFiles: number;
  /** Synthetic corpus size for the scale lane. */
  readonly scaleCorpusFiles: number;
  /** Receipt destination (file path or directory), if set. */
  readonly receiptPath: string | null;
}

/** The non-vacuity receipt every scenario run emits. */
export interface EnduranceReceipt {
  readonly schemaVersion: 1;
  readonly scenario: string;
  readonly route: EnduranceProviderRoute;
  readonly startedAt: string;
  readonly durationMs: number;
  readonly requestsSent: number;
  readonly requestsAnswered: number;
  readonly requestsCancelled: number;
  readonly requestsErrored: number;
  readonly requestsUnanswered: number;
  /** didChange notifications issued (typing/edit load, not requests). */
  readonly editsSent: number;
  readonly latency: {
    readonly overall: PercentileSummary;
    readonly windows: readonly WindowSummary[];
  };
  readonly maxRssBytes: number | null;
  readonly rssSupported: boolean;
  readonly providerAliveAtEnd: boolean;
  /** Post-load full-feature sanity pass (hover+completion+definition). */
  readonly finalSanityPass: boolean | null;
  /** Soak degradation verdict; null when <2 usable windows (too short). */
  readonly degradationCheck: {
    readonly earlyWindowP95: number;
    readonly lateWindowP95: number;
    readonly factor: number;
    readonly pass: boolean;
  } | null;
  /**
   * INFORMATIONAL type-quality observations over every answered
   * hover/completion — the documented provider type-quality backlog
   * (any-typed hovers, empty member completions), recorded as data, never
   * asserted by the stability harness.
   */
  readonly typeQuality: {
    readonly hovers: { readonly total: number; readonly empty: number; readonly anyTyped: number };
    readonly completions: { readonly total: number; readonly empty: number };
  };
  /** Assertion-relevant config echoed for auditability. */
  readonly config: {
    readonly p95MaxMs: number;
    readonly stormP95MaxMs: number;
    readonly rssMaxBytes: number;
    readonly requestTimeoutMs: number;
  };
  /**
   * INFORMATIONAL edit-pipeline measurement (never asserted): the offered
   * didChange rate, the server-side did_change handler cost parsed from the
   * server's stderr HANDLER_EXIT lines, and the resulting pipeline
   * utilization. At superhuman typing rates this is the throughput ceiling;
   * at the default human cadence (VERTER_ENDURANCE_TYPING_CPS=12) it should
   * sit well below 1.
   */
  readonly throughputCeiling: {
    readonly editsPerSecond: number;
    readonly didChangeHandlerMs: {
      readonly samples: number;
      readonly p50: number;
      readonly max: number;
    } | null;
    readonly pipelineUtilization: number | null;
  };
  /** Content-mismatch / correctness failures observed (capped). */
  readonly failures: readonly string[];
}
