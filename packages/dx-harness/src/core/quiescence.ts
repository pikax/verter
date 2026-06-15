/**
 * The shared quiescence decision for the Verter DX startup gates.
 *
 * After the init-generation gate matches (see {@link ./generationGate}), the
 * server may still be churning: the workspace scanner drains, the type provider
 * syncs, diagnostics re-publish. Probing before that settles makes churn
 * unattributable to a later edit. The harness therefore waits for QUIESCENCE.
 *
 * The host exposes monotonically-increasing work counters through
 * `$/verter/getStatistics` — `host:compile`, `host:upsert`, `host:cache_hits`
 * (`StatisticsSnapshot.session.byType[...].count`, emitted at
 * crates/verter_lsp/src/server/custom_methods/mod.rs:398-433). The decision is:
 * the host is quiesced when those three counters hold UNCHANGED for two
 * consecutive polling intervals AND no new scanner/drain/sync WARN line appears
 * in the same window. A diagnostics-aware variant additionally requires the
 * diagnostics fingerprint to hold and provider queries to succeed.
 *
 * The decision ({@link decideQuiescence}) is a pure function over samples so it is
 * discriminating-testable; {@link pollUntilQuiesced} is the thin polling wrapper.
 * Importing this module does no I/O and mutates no globals.
 */

/** The three host work counters quiescence watches. */
export interface QuiescenceCounters {
  readonly compile: number;
  readonly upsert: number;
  readonly cacheHits: number;
}

/** The `session.byType` keys the host emits these counters under. */
export const QUIESCENCE_COUNTER_KEYS = {
  compile: "host:compile",
  upsert: "host:upsert",
  cacheHits: "host:cache_hits",
} as const;

function readCount(byType: unknown, key: string): number {
  const entry = (byType as Record<string, unknown> | null | undefined)?.[key];
  const count = (entry as { count?: unknown } | null | undefined)?.count;
  return typeof count === "number" && Number.isFinite(count) ? count : 0;
}

/**
 * Project a `$/verter/getStatistics` snapshot down to the three quiescence
 * counters. Defensive: a missing counter, a missing `session`, or a malformed
 * snapshot all read as `0` rather than throwing — a poll must never crash the
 * gate. Only `session.byType` host counters are read; `byFile` is ignored.
 */
export function extractQuiescenceCounters(snapshot: unknown): QuiescenceCounters {
  const byType = (snapshot as { session?: { byType?: unknown } } | null | undefined)?.session
    ?.byType;
  return {
    compile: readCount(byType, QUIESCENCE_COUNTER_KEYS.compile),
    upsert: readCount(byType, QUIESCENCE_COUNTER_KEYS.upsert),
    cacheHits: readCount(byType, QUIESCENCE_COUNTER_KEYS.cacheHits),
  };
}

/** Whether two counter samples are identical across all three counters. */
export function countersEqual(a: QuiescenceCounters, b: QuiescenceCounters): boolean {
  return a.compile === b.compile && a.upsert === b.upsert && a.cacheHits === b.cacheHits;
}

/** Keyword set whose WARN lines reset quiescence (scanner / drain / sync churn). */
export const QUIESCENCE_WARN_KEYWORDS = ["scanner", "drain", "sync"] as const;

// The tracing level token is uppercase (`WARN` / `WARNING`); requiring it avoids
// matching the lowercase word "warning" inside an INFO message body. The keyword
// match below is case-insensitive against the target/message.
const WARN_LEVEL_PATTERN = /(?:^|[^A-Za-z])WARN(?:ING)?(?:[^A-Za-z]|$)/;

/**
 * Whether a stderr line is a scanner/drain/sync WARN that should reset
 * quiescence. Requires BOTH a WARN-level token and one of {@link keywords}.
 */
export function isQuiescenceWarnLine(
  line: string,
  keywords: readonly string[] = QUIESCENCE_WARN_KEYWORDS,
): boolean {
  if (!WARN_LEVEL_PATTERN.test(line)) return false;
  const lower = line.toLowerCase();
  return keywords.some((keyword) => lower.includes(keyword.toLowerCase()));
}

/**
 * One polling observation: the counter sample, the (already scanner/drain/sync
 * filtered) WARN lines that arrived since the previous observation, and the
 * optional diagnostics-variant inputs.
 */
export interface QuiescenceObservation {
  readonly counters: QuiescenceCounters;
  readonly newWarnLines: readonly string[];
  /**
   * Fingerprint of the published diagnostics at this sample. Compared between
   * neighbours only when present on both — omit it for the counter-only variant.
   */
  readonly diagnosticsFingerprint?: string | null;
  /**
   * Whether a provider probe succeeded at this sample. When explicitly `false`
   * the interval is unstable; omit it for the counter-only variant.
   */
  readonly providerQueryOk?: boolean;
}

/** The quiescence verdict over a sequence of observations. */
export interface QuiescenceDecision {
  readonly quiesced: boolean;
  /** Consecutive stable intervals counting back from the most recent. */
  readonly stableIntervals: number;
  readonly requiredStableIntervals: number;
  /** Why it is not quiesced; empty when quiesced. */
  readonly reason: string;
}

/** Default: counters must hold across two consecutive polling intervals. */
export const REQUIRED_STABLE_INTERVALS = 2;

type IntervalStatus =
  | { readonly stable: true }
  | { readonly stable: false; readonly reason: string };

/** Classify the interval between two adjacent observations. */
function intervalStatus(prev: QuiescenceObservation, curr: QuiescenceObservation): IntervalStatus {
  if (!countersEqual(prev.counters, curr.counters)) {
    return { stable: false, reason: "host counters still changing" };
  }
  if (curr.newWarnLines.length > 0) {
    return {
      stable: false,
      reason: `new scanner/drain/sync warn line(s): ${curr.newWarnLines.length}`,
    };
  }
  if (
    prev.diagnosticsFingerprint != null &&
    curr.diagnosticsFingerprint != null &&
    prev.diagnosticsFingerprint !== curr.diagnosticsFingerprint
  ) {
    return { stable: false, reason: "diagnostics still changing" };
  }
  if (curr.providerQueryOk === false) {
    return { stable: false, reason: "provider query failing" };
  }
  return { stable: true };
}

/**
 * Pure quiescence decision. The host is quiesced when the most recent
 * `requiredStableIntervals` intervals are ALL stable — counters unchanged, no new
 * scanner/drain/sync WARN line, and (when supplied) diagnostics unchanged and the
 * provider probe succeeding. Any churn resets the trailing stable run.
 */
export function decideQuiescence(
  observations: readonly QuiescenceObservation[],
  requiredStableIntervals: number = REQUIRED_STABLE_INTERVALS,
): QuiescenceDecision {
  const required = Math.max(1, Math.floor(requiredStableIntervals));
  if (observations.length < 2) {
    return {
      quiesced: false,
      stableIntervals: 0,
      requiredStableIntervals: required,
      reason: "insufficient samples (need at least two)",
    };
  }

  let stableIntervals = 0;
  let blockingReason = "";
  for (let i = observations.length - 1; i >= 1; i--) {
    const status = intervalStatus(observations[i - 1], observations[i]);
    if (status.stable) {
      stableIntervals++;
    } else {
      blockingReason = status.reason;
      break;
    }
  }

  const quiesced = stableIntervals >= required;
  const reason = quiesced
    ? ""
    : blockingReason || `only ${stableIntervals}/${required} stable interval(s) observed`;
  return { quiesced, stableIntervals, requiredStableIntervals: required, reason };
}

/** Options for {@link pollUntilQuiesced}; clock hooks are injectable for tests. */
export interface PollUntilQuiescedOptions {
  /** Delay between polls (ms). Default 150. */
  readonly intervalMs?: number;
  readonly requiredStableIntervals?: number;
  /** Overall budget (ms) before giving up with `timedOut`. Default 15000. */
  readonly timeoutMs?: number;
  /** Sleep primitive — injected as immediate in tests. */
  readonly sleep?: (ms: number) => Promise<void>;
  /** Monotonic clock — injected as a manual clock in tests. */
  readonly now?: () => number;
  /** Abort the poll loop cooperatively. */
  readonly signal?: AbortSignal;
}

/** The outcome of a {@link pollUntilQuiesced} run. */
export interface QuiescenceResult {
  readonly quiesced: boolean;
  readonly timedOut: boolean;
  readonly aborted: boolean;
  readonly pollCount: number;
  readonly decision: QuiescenceDecision;
  readonly observations: readonly QuiescenceObservation[];
}

const DEFAULT_POLL_INTERVAL_MS = 150;
const DEFAULT_QUIESCENCE_TIMEOUT_MS = 15_000;

function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    timer.unref?.();
  });
}

/**
 * Drive {@link decideQuiescence} by repeatedly sampling `pollCounters` and
 * `drainWarnLines` (which returns the scanner/drain/sync WARN lines seen since the
 * previous call) until quiescence, timeout, or abort. The first observation's
 * warn lines are pre-window and never block — they only flush the warn cursor.
 *
 * `timeoutMs` is a hard wall-clock cap: the inter-poll sleep is clamped to the
 * remaining budget, so the loop never overshoots the deadline by a poll interval
 * even when `intervalMs` is oversized or does not divide the budget.
 */
export async function pollUntilQuiesced(
  pollCounters: () => Promise<QuiescenceCounters>,
  drainWarnLines: () => readonly string[],
  options: PollUntilQuiescedOptions = {},
): Promise<QuiescenceResult> {
  const intervalMs = options.intervalMs ?? DEFAULT_POLL_INTERVAL_MS;
  const required = options.requiredStableIntervals ?? REQUIRED_STABLE_INTERVALS;
  const timeoutMs = options.timeoutMs ?? DEFAULT_QUIESCENCE_TIMEOUT_MS;
  const sleep = options.sleep ?? defaultSleep;
  const now = options.now ?? Date.now;
  const signal = options.signal;

  const start = now();
  const observations: QuiescenceObservation[] = [];
  let decision = decideQuiescence(observations, required);

  for (;;) {
    if (signal?.aborted) {
      return {
        quiesced: false,
        timedOut: false,
        aborted: true,
        pollCount: observations.length,
        decision,
        observations,
      };
    }

    const counters = await pollCounters();
    const newWarnLines = [...drainWarnLines()];
    observations.push({ counters, newWarnLines });
    decision = decideQuiescence(observations, required);

    if (decision.quiesced) {
      return {
        quiesced: true,
        timedOut: false,
        aborted: false,
        pollCount: observations.length,
        decision,
        observations,
      };
    }
    // One clock read per iteration: the manual clocks tests inject advance on
    // every read, so reusing `elapsed` for both the timeout check and the sleep
    // cap keeps the loop's observable timing identical to a single now() call.
    const elapsed = now() - start;
    if (elapsed >= timeoutMs) {
      return {
        quiesced: false,
        timedOut: true,
        aborted: false,
        pollCount: observations.length,
        decision,
        observations,
      };
    }
    // Clamp the inter-poll sleep to the remaining budget. `timeoutMs` is a HARD
    // wall-clock cap (awaitRawLspStartup sets it to the residual ready budget);
    // sleeping the full `intervalMs` here would let an oversized or non-divisible
    // interval carry the clock PAST the deadline before the next iteration's
    // timeout check runs. `elapsed < timeoutMs` holds (we returned otherwise), so
    // the clamp is in (0, intervalMs].
    await sleep(Math.min(intervalMs, timeoutMs - elapsed));
  }
}
