/**
 * Raw-LSP startup-readiness gate.
 *
 * Drives a `@verter/lsp-test-client` `LspClient` (or any structural
 * {@link StartupLspClient}) to the point where the first cross-file probe is
 * reliable and churn is attributable to a later edit:
 *
 *  1. Observe `$/verter/ready` and `$/verter/typeProviderSyncComplete`, matching
 *     them per init generation through the shared {@link ./generationGate}. The
 *     gate proceeds only once BOTH have arrived for the same — newest —
 *     generation; `ready` alone is non-semantic and never advances the gate.
 *  2. Once a generation matches, run the {@link ./quiescence} gate against
 *     `$/verter/getStatistics` (host counters) and the buffered stderr WARN feed.
 *  3. If a newer generation supersedes mid-quiescence, re-arm and follow it
 *     (newest-wins), bounded by the overall ready timeout.
 *
 * The generation-matching and quiescence cores are pure and exhaustively tested;
 * this wrapper is the thin async orchestration that wires them to a live client.
 */
import { GenerationGate, type GenerationGateDecision } from "./generationGate.js";
import {
  QUIESCENCE_WARN_KEYWORDS,
  extractQuiescenceCounters,
  isQuiescenceWarnLine,
  pollUntilQuiesced,
  type PollUntilQuiescedOptions,
  type QuiescenceCounters,
  type QuiescenceResult,
} from "./quiescence.js";

/** Server→client readiness notification methods (protocol_types.rs:58,73). */
export const VERTER_READY_METHOD = "$/verter/ready";
export const TYPE_PROVIDER_SYNC_COMPLETE_METHOD = "$/verter/typeProviderSyncComplete";
/** Host statistics request (custom_methods/mod.rs:360). */
export const GET_STATISTICS_METHOD = "$/verter/getStatistics";

/**
 * The narrow slice of `@verter/lsp-test-client`'s `LspClient` the gate consumes.
 * `LspClient` structurally satisfies it; tests pass an in-memory fake.
 */
export interface StartupLspClient {
  onNotification(method: string, handler: (params: any) => void): void;
  offNotification(method: string, handler: (params: any) => void): void;
  sendRequest<T = any>(method: string, params?: any, timeout?: number): Promise<T>;
  readonly stderr: { text(): string };
}

/** Extract the `gen` field from a readiness notification payload. */
function readGeneration(params: unknown): number | null {
  const gen = (params as { gen?: unknown } | null | undefined)?.gen;
  return typeof gen === "number" && Number.isInteger(gen) && gen >= 0 ? gen : null;
}

/** EOL matcher mirroring `StderrBuffer`'s line splitter (lsp-test-client). */
const STDERR_EOL = /\r\n|\n|\r/;

/**
 * Build a drainer over a child's retained stderr that, on each call, returns the
 * scanner/drain/sync WARN lines that COMPLETED since the previous call.
 *
 * The buffered stderr exposes `text()` whose tail may be an UNTERMINATED partial
 * line (a chunk boundary can split a single log line — `"WARN workspace_"` then
 * `"scanner busy\n"`). The drainer carries that partial across calls by advancing
 * its char cursor only over completed lines, so a WARN whose text straddles two
 * chunks is observed exactly once — when it completes — never dropped, never split.
 *
 * Assumes the buffer retains the window (`StderrBuffer` default `maxBytes =
 * Infinity`), which the harness configures for the startup window; a `clear()`
 * that shrinks the buffer below the cursor restarts it.
 */
export function createWarnLineDrainer(
  stderr: { text(): string },
  keywords: readonly string[] = QUIESCENCE_WARN_KEYWORDS,
): () => string[] {
  // A CHAR offset (not a line index) into the retained stderr, advanced only over
  // completed lines. Counting lines instead would advance past an unterminated
  // partial, then slice the completed line away on the next drain.
  let cursor = 0;
  return () => {
    const text = stderr.text();
    if (cursor > text.length) cursor = 0; // buffer was cleared/trimmed below us
    const parts = text.slice(cursor).split(STDERR_EOL);
    // The final element is the unterminated trailing partial; leave it unconsumed
    // so it is re-read and observed exactly once when a later chunk completes it.
    const partial = parts.pop() ?? "";
    cursor = text.length - partial.length;
    return parts.filter((line) => isQuiescenceWarnLine(line, keywords));
  };
}

/** Options for {@link awaitRawLspStartup}; clock hooks are injectable for tests. */
export interface AwaitRawLspStartupOptions {
  /** Total budget to reach a quiesced matched generation (ms). Default 60000. */
  readonly readyTimeoutMs?: number;
  /** Per-request timeout for `$/verter/getStatistics` (ms). Default 10000. */
  readonly statisticsTimeoutMs?: number;
  /** WARN keywords that reset quiescence. Default scanner/drain/sync. */
  readonly warnKeywords?: readonly string[];
  /** Quiescence tuning; clock/abort hooks are taken from the top-level options. */
  readonly quiescence?: Omit<PollUntilQuiescedOptions, "signal" | "now" | "sleep">;
  readonly signal?: AbortSignal;
  readonly sleep?: (ms: number) => Promise<void>;
  readonly now?: () => number;
}

/** The result of a successful {@link awaitRawLspStartup}. */
export interface RawLspStartupResult {
  readonly matchedGeneration: number;
  readonly generation: GenerationGateDecision;
  readonly quiescence: QuiescenceResult;
}

// Mirrors the extension's 60s background-init grace (extension.ts:846-847).
const DEFAULT_READY_TIMEOUT_MS = 60_000;
const DEFAULT_STATISTICS_TIMEOUT_MS = 10_000;

/**
 * Resolve once {@link GenerationGate.satisfied} holds, or reject at `deadline`.
 * `setWake` installs a re-check callback the notification handlers invoke.
 */
function waitForMatchedGeneration(
  gate: GenerationGate,
  deadline: number,
  now: () => number,
  setWake: (fn: (() => void) | null) => void,
  signal?: AbortSignal,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    if (signal?.aborted) {
      reject(new Error("raw LSP startup aborted while awaiting matched init generation"));
      return;
    }
    if (gate.satisfied) {
      resolve();
      return;
    }
    const cleanup = () => {
      clearTimeout(timer);
      setWake(null);
      signal?.removeEventListener("abort", onAbort);
    };
    const onAbort = () => {
      cleanup();
      reject(new Error("raw LSP startup aborted while awaiting matched init generation"));
    };
    const check = () => {
      if (gate.satisfied) {
        cleanup();
        resolve();
      }
    };
    const timer = setTimeout(
      () => {
        cleanup();
        reject(
          new Error(
            "timed out waiting for matched init generation (ready + typeProviderSyncComplete)",
          ),
        );
      },
      Math.max(0, deadline - now()),
    );
    timer.unref?.();
    signal?.addEventListener("abort", onAbort);
    setWake(check);
  });
}

/**
 * Await raw-LSP startup readiness: a matched ready+sync generation followed by
 * host quiescence. Rejects if neither is reached within `readyTimeoutMs`.
 */
export async function awaitRawLspStartup(
  client: StartupLspClient,
  options: AwaitRawLspStartupOptions = {},
): Promise<RawLspStartupResult> {
  const readyTimeoutMs = options.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS;
  const statisticsTimeoutMs = options.statisticsTimeoutMs ?? DEFAULT_STATISTICS_TIMEOUT_MS;
  const warnKeywords = options.warnKeywords ?? QUIESCENCE_WARN_KEYWORDS;
  const now = options.now ?? Date.now;
  const { sleep, signal } = options;

  // `readyTimeoutMs` is the TOTAL budget to reach a quiesced matched generation.
  // Every wait below is bounded by what remains of it, so a match arriving near
  // the deadline cannot overrun by waiting the full quiescence timeout or a full
  // `getStatistics` request timeout.
  const deadline = now() + readyTimeoutMs;
  const remainingBudget = (): number => Math.max(0, deadline - now());

  const gate = new GenerationGate();
  let wake: (() => void) | null = null;
  const setWake = (fn: (() => void) | null) => {
    wake = fn;
  };

  const onReady = (params: unknown) => {
    const generation = readGeneration(params);
    if (generation !== null) gate.observeReady(generation);
    wake?.();
  };
  const onSync = (params: unknown) => {
    const generation = readGeneration(params);
    if (generation !== null) gate.observeSync(generation);
    wake?.();
  };

  // Warn-line draining over buffered stderr. The drainer carries the unterminated
  // trailing partial across calls so a WARN split across stderr chunks is observed
  // exactly once (see {@link createWarnLineDrainer}).
  const drainWarnLines = createWarnLineDrainer(client.stderr, warnKeywords);

  const pollCounters = async (): Promise<QuiescenceCounters> => {
    const remaining = remainingBudget();
    if (remaining <= 0) {
      throw new Error(
        `raw LSP startup exceeded its ${readyTimeoutMs}ms total budget before host quiescence`,
      );
    }
    // Never let a single statistics request outlive the total budget.
    const requestTimeout = Math.min(remaining, statisticsTimeoutMs);
    const snapshot = await client.sendRequest(GET_STATISTICS_METHOD, {}, requestTimeout);
    return extractQuiescenceCounters(snapshot);
  };

  client.onNotification(VERTER_READY_METHOD, onReady);
  client.onNotification(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, onSync);
  // Discard pre-subscription warns so only warns inside quiescence windows count.
  drainWarnLines();

  try {
    for (;;) {
      if (signal?.aborted) throw new Error("raw LSP startup aborted");

      await waitForMatchedGeneration(gate, deadline, now, setWake, signal);
      const matched = gate.matchedGeneration;
      if (matched === null) continue; // raced re-arm; re-await

      // Cap the quiescence poll to the remaining budget so it cannot overrun the
      // total deadline even when a larger quiescence.timeoutMs is configured.
      const remaining = remainingBudget();
      const quiescence = await pollUntilQuiesced(pollCounters, drainWarnLines, {
        ...options.quiescence,
        timeoutMs: Math.min(remaining, options.quiescence?.timeoutMs ?? remaining),
        signal,
        sleep,
        now,
      });

      // A newer generation may have superseded `matched` while polling. Only
      // accept the result if the matched generation is still current AND quiesced.
      if (gate.matchedGeneration === matched && quiescence.quiesced) {
        return { matchedGeneration: matched, generation: gate.decision, quiescence };
      }
      if (now() >= deadline) {
        throw new Error(
          `raw LSP startup did not reach a quiesced matched generation within ${readyTimeoutMs}ms ` +
            `(generation=${JSON.stringify(gate.decision)}, quiesced=${quiescence.quiesced})`,
        );
      }
      // Otherwise loop: re-await the (possibly newer) matched generation and re-run
      // quiescence — newest-wins, bounded by the overall deadline.
    }
  } finally {
    client.offNotification(VERTER_READY_METHOD, onReady);
    client.offNotification(TYPE_PROVIDER_SYNC_COMPLETE_METHOD, onSync);
    wake = null;
  }
}
