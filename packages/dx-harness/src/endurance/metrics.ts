/**
 * Metrics substrate for the endurance harness: per-request latency recording
 * with overall + time-window percentiles, the send/settle counters behind the
 * "zero unanswered" gate, request-error classification, and the bounded
 * in-flight pool the harness uses so it never builds unbounded queues.
 */
import type { PercentileSummary, RequestClassification, WindowSummary } from "./types.js";

export interface LatencySample {
  readonly method: string;
  readonly latencyMs: number;
  readonly ok: boolean;
  readonly windowIndex: number;
  /** Milliseconds since recorder start. */
  readonly at: number;
}

/** Nearest-rank percentile over an unsorted list; 0 for empty input. */
export function percentileOf(values: readonly number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.min(sorted.length, Math.max(1, Math.ceil((p / 100) * sorted.length)));
  return sorted[rank - 1];
}

export function summarize(values: readonly number[]): PercentileSummary {
  if (values.length === 0) {
    return { count: 0, p50: 0, p95: 0, p99: 0, max: 0, mean: 0 };
  }
  let sum = 0;
  let max = 0;
  for (const value of values) {
    sum += value;
    if (value > max) max = value;
  }
  return {
    count: values.length,
    p50: percentileOf(values, 50),
    p95: percentileOf(values, 95),
    p99: percentileOf(values, 99),
    max,
    mean: sum / values.length,
  };
}

/**
 * Records one latency sample per request and computes percentiles overall and
 * per fixed time window (windowIndex = elapsed / windowMs) for trend analysis.
 */
export class LatencyRecorder {
  private readonly samples: LatencySample[] = [];
  private readonly startedAt: number;

  constructor(
    readonly windowMs: number,
    private readonly now: () => number = Date.now,
  ) {
    this.startedAt = now();
  }

  record(method: string, latencyMs: number, ok: boolean): LatencySample {
    const at = this.now() - this.startedAt;
    const sample: LatencySample = {
      method,
      latencyMs,
      ok,
      windowIndex: Math.floor(at / this.windowMs),
      at,
    };
    this.samples.push(sample);
    return sample;
  }

  get count(): number {
    return this.samples.length;
  }

  overall(): PercentileSummary {
    return summarize(this.samples.map((sample) => sample.latencyMs));
  }

  windows(): WindowSummary[] {
    const byWindow = new Map<number, number[]>();
    for (const sample of this.samples) {
      const bucket = byWindow.get(sample.windowIndex) ?? [];
      bucket.push(sample.latencyMs);
      byWindow.set(sample.windowIndex, bucket);
    }
    return [...byWindow.entries()]
      .sort(([left], [right]) => left - right)
      .map(([windowIndex, values]) => ({
        ...summarize(values),
        windowIndex,
        startedAtMs: windowIndex * this.windowMs,
      }));
  }

  /**
   * Compare the first and last windows that carry at least `minSamples`
   * requests. Returns null when fewer than two usable windows exist (a run too
   * short for trend analysis — the caller must report that honestly, not pass
   * vacuously).
   *
   * A MEANINGFUL degradation requires BOTH signals: the late window exceeds
   * `early * factor` (relative) AND `early + floorMs` (absolute). On a
   * fast-baseline route a sub-floor wiggle (e.g. 66→114ms) is run-to-run
   * noise, not a trend; a genuine climb (ratio > factor AND delta > floor)
   * still fails.
   *
   * `minSamples = 40` is the statistical-sufficiency floor for a p95 VERDICT:
   * a 95th-percentile estimate over a handful of samples is dominated by one
   * or two tail spikes, so on CI-sized soaks (e.g. ~20 samples per 1s window)
   * first/last-window comparisons false-positive on warmup spikes. The
   * default long soak (30s windows, hundreds of samples each) keeps the real
   * verdict; a shrunk run reports null (logged, never a vacuous pass).
   */
  degradation(
    factor: number,
    floorMs: number,
    minSamples = 40,
  ): {
    earlyWindowP95: number;
    lateWindowP95: number;
    factor: number;
    floorMs: number;
    pass: boolean;
  } | null {
    const usable = this.windows().filter((window) => window.count >= minSamples);
    if (usable.length < 2) return null;
    const early = usable[0];
    const late = usable[usable.length - 1];
    return {
      earlyWindowP95: early.p95,
      lateWindowP95: late.p95,
      factor,
      floorMs,
      pass: late.p95 <= early.p95 * factor || late.p95 - early.p95 <= floorMs,
    };
  }
}

/**
 * Counters behind the attestation's non-vacuity gates. Invariant:
 * `sent === answered + cancelled + errored + unanswered`.
 */
export class RequestTracker {
  sent = 0;
  answered = 0;
  cancelled = 0;
  errored = 0;
  unanswered = 0;
  /** didChange notifications sent (edit load; no response expected). */
  editsSent = 0;

  settle(classification: Exclude<RequestClassification, never>): void {
    this[classification] += 1;
  }

  /** Assertable invariant: every sent request reached a terminal settle. */
  get settledTotal(): number {
    return this.answered + this.cancelled + this.errored + this.unanswered;
  }
}

/**
 * Classify a rejected `LspClient.sendRequest` promise.
 *
 * The client's rejection messages are stable (lspClient.ts):
 *  - timeout:        `"<name> request '<method>' timed out after <ms>ms"`
 *  - LSP error resp: `"<name> LSP error: {\"code\":...,\"message\":...}"`
 *  - transport:      process exited / not running / stdin not writable
 *
 * A timeout or transport failure means NO protocol answer arrived → the drop
 * this harness must catch → `unanswered`. A cancellation error response is a
 * proper answer → `cancelled`. Any other error response → `errored`.
 */
export function classifyRequestError(err: unknown): RequestClassification {
  const message = err instanceof Error ? err.message : String(err);
  const lspError = /LSP error: (\{.*\})/.exec(message);
  if (lspError) {
    try {
      const parsed = JSON.parse(lspError[1]) as { code?: unknown };
      // -32800 RequestCancelled, -32801 ContentModified: proper cancellations.
      if (parsed.code === -32800 || parsed.code === -32801) return "cancelled";
      return "errored";
    } catch {
      return "errored";
    }
  }
  return "unanswered";
}

/**
 * A counting semaphore bounding concurrent in-flight work. `run` waits for a
 * slot up to `acquireTimeoutMs`, then rejects — a saturated pool means the
 * server is not draining requests, which the caller counts as unanswered.
 */
export class ConcurrencyPool {
  private inFlight = 0;
  private readonly waiters: Array<{
    resolve: () => void;
    reject: (err: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  }> = [];

  constructor(
    readonly max: number,
    private readonly acquireTimeoutMs: number,
  ) {
    if (max < 1) throw new Error(`ConcurrencyPool max must be >= 1, got ${max}`);
  }

  async run<T>(fn: () => Promise<T>): Promise<T> {
    await this.acquire();
    try {
      return await fn();
    } finally {
      this.release();
    }
  }

  private acquire(): Promise<void> {
    if (this.inFlight < this.max) {
      this.inFlight += 1;
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      const waiter = {
        resolve: () => {
          clearTimeout(waiter.timer);
          this.inFlight += 1;
          resolve();
        },
        reject,
        timer: setTimeout(() => {
          const index = this.waiters.indexOf(waiter);
          if (index >= 0) this.waiters.splice(index, 1);
          reject(
            new Error(
              `endurance harness pool saturated: no in-flight slot freed within ${this.acquireTimeoutMs}ms`,
            ),
          );
        }, this.acquireTimeoutMs),
      };
      waiter.timer.unref?.();
      this.waiters.push(waiter);
    });
  }

  private release(): void {
    this.inFlight -= 1;
    const next = this.waiters.shift();
    if (next) {
      clearTimeout(next.timer);
      next.resolve();
    }
  }
}

/**
 * Parse server-side handler costs from verter-lsp's stderr `HANDLER_EXIT
 * <handler> active=N elapsed=<v><unit>` lines (units: `ms` or `s`). Used for
 * the receipt's INFORMATIONAL throughputCeiling measurement.
 */
export function parseHandlerExitCostsMs(stderr: string, handler: string): number[] {
  const pattern = new RegExp(`HANDLER_EXIT ${handler} active=\\d+ elapsed=([0-9.]+)(ms|s)`, "g");
  const costs: number[] = [];
  for (const match of stderr.matchAll(pattern)) {
    const value = Number(match[1]);
    if (Number.isFinite(value)) costs.push(match[2] === "s" ? value * 1000 : value);
  }
  return costs;
}

/**
 * INFORMATIONAL type-quality observations — the documented, pre-existing
 * provider type-quality gaps (any-typed hovers, empty member completions) are
 * tracked here as DATA, never asserted. The stability contract (settle /
 * alive / latency / RSS / D1 prop-name completion / definition mapping) is
 * asserted at full strength elsewhere; this field exists so a receipt shows
 * the type-quality state without re-litigating the known backlog.
 */
export interface TypeQualitySnapshot {
  readonly hovers: { readonly total: number; readonly empty: number; readonly anyTyped: number };
  readonly completions: { readonly total: number; readonly empty: number };
}

export class TypeQualityRecorder {
  private hoverTotal = 0;
  private hoverEmpty = 0;
  private hoverAny = 0;
  private completionTotal = 0;
  private completionEmpty = 0;

  recordHover(text: string): void {
    this.hoverTotal += 1;
    if (text.trim().length === 0) this.hoverEmpty += 1;
    else if (/\bany\b/.test(text)) this.hoverAny += 1;
  }

  recordCompletion(labels: readonly string[]): void {
    this.completionTotal += 1;
    if (labels.length === 0) this.completionEmpty += 1;
  }

  snapshot(): TypeQualitySnapshot {
    return {
      hovers: { total: this.hoverTotal, empty: this.hoverEmpty, anyTyped: this.hoverAny },
      completions: { total: this.completionTotal, empty: this.completionEmpty },
    };
  }
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    timer.unref?.();
  });
}
