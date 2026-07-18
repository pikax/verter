/**
 * Shared machinery for the endurance scenario runners: failure collection,
 * keystroke-level typing drivers with probe checkpoints, and receipt assembly.
 */
import type { RssSampler } from "../rss.js";
import type { EnduranceSession, EnduranceProbe } from "../session.js";
import { parseHandlerExitCostsMs, percentileOf, sleep } from "../metrics.js";
import type { EnduranceConfig, EnduranceProviderRoute, EnduranceReceipt } from "../types.js";

/** A bounded failure collector: the run reports, then asserts on, ALL of them. */
export class FailureBag {
  private readonly failures: string[] = [];

  constructor(private readonly cap = 20) {}

  add(failure: string): void {
    if (this.failures.length < this.cap) this.failures.push(failure);
  }

  get list(): readonly string[] {
    return this.failures;
  }
}

/** Replace the first occurrence of `from`, throwing when it is absent (fixture drift). */
export function replaceOnce(text: string, from: string, to: string): string {
  const index = text.indexOf(from);
  if (index === -1) {
    throw new Error(`replaceOnce: pattern not found: ${JSON.stringify(from)}`);
  }
  return text.slice(0, index) + to + text.slice(index + from.length);
}

export interface ScenarioContext {
  readonly scenario: string;
  readonly route: EnduranceProviderRoute;
  readonly session: EnduranceSession;
  readonly config: EnduranceConfig;
  readonly sampler: RssSampler | null;
}

/**
 * A probe to run when the typed region reaches an exact length. `makeProbe`
 * receives the typed-so-far text so the probe needle can pin the cursor to
 * exactly the end of the typed region.
 */
export interface TypingCheckpoint {
  readonly atLength: number;
  readonly makeProbe: (typedSoFar: string) => EnduranceProbe;
}

/** Run one checkpoint probe; record (never throw) classification/content/latency failures. */
export async function runCheckpoint(
  context: ScenarioContext,
  probe: EnduranceProbe,
  failures: FailureBag,
): Promise<void> {
  let outcome;
  try {
    outcome = await context.session.runProbe(probe, context.config.probeTimeoutMs);
  } catch (error) {
    failures.add(`probe ${probe.label} threw: ${error instanceof Error ? error.message : error}`);
    return;
  }
  if (outcome.classification !== "answered") {
    failures.add(`probe ${probe.label} settled as ${outcome.classification}`);
    return;
  }
  if (outcome.mismatch) failures.add(outcome.mismatch);
  if (outcome.latencyMs > context.config.probeLatencyBoundMs) {
    failures.add(
      `probe ${probe.label} latency ${outcome.latencyMs}ms exceeds bound ${context.config.probeLatencyBoundMs}ms`,
    );
  }
}

/**
 * Assert a probe's response CONVERGES to the expected content after an edit.
 *
 * LSP edit propagation is asynchronous: a request issued immediately after a
 * didChange may legitimately observe a pre-edit snapshot. The honest contract
 * is convergence — the server must produce the correct answer within a hard
 * deadline, and every response along the way must settle (answered) and stay
 * under the latency bound. A settle failure is reported immediately (that is
 * a drop, not staleness); only content mismatches are retried.
 */
export async function convergeProbe(
  context: ScenarioContext,
  probe: EnduranceProbe,
  failures: FailureBag,
  options: { timeoutMs?: number; intervalMs?: number } = {},
): Promise<boolean> {
  const timeoutMs = options.timeoutMs ?? context.config.probeLatencyBoundMs;
  const intervalMs = options.intervalMs ?? 100;
  const deadline = Date.now() + timeoutMs;
  let lastMismatch: string | null = null;
  for (;;) {
    let outcome;
    try {
      outcome = await context.session.runProbe(probe, context.config.probeTimeoutMs);
    } catch (error) {
      failures.add(`probe ${probe.label} threw: ${error instanceof Error ? error.message : error}`);
      return false;
    }
    if (outcome.classification !== "answered") {
      failures.add(`probe ${probe.label} settled as ${outcome.classification}`);
      return false;
    }
    if (outcome.latencyMs > context.config.probeLatencyBoundMs) {
      failures.add(
        `probe ${probe.label} latency ${outcome.latencyMs}ms exceeds bound ${context.config.probeLatencyBoundMs}ms`,
      );
    }
    if (!outcome.mismatch) return true;
    lastMismatch = outcome.mismatch;
    if (Date.now() >= deadline) {
      failures.add(`probe ${probe.label} did not converge within ${timeoutMs}ms: ${lastMismatch}`);
      return false;
    }
    await sleep(intervalMs);
  }
}

function sortedCheckpoints(checkpoints: readonly TypingCheckpoint[]): TypingCheckpoint[] {
  const sorted = [...checkpoints].sort((a, b) => a.atLength - b.atLength);
  for (const checkpoint of sorted) {
    if (checkpoint.atLength < 1) {
      throw new Error(`checkpoint atLength must be >= 1, got ${checkpoint.atLength}`);
    }
  }
  return sorted;
}

/**
 * Keystroke-level typing of a fresh buffer: the document is always a PREFIX
 * of `finalText`, one didChange per small chunk at the configured HUMAN
 * cadence (VERTER_ENDURANCE_TYPING_CPS, default 12/s), pausing at exact
 * checkpoint lengths to run probes against the just-typed prefix.
 */
export async function typeFromScratch(
  context: ScenarioContext,
  relativePath: string,
  finalText: string,
  checkpoints: readonly TypingCheckpoint[],
  failures: FailureBag,
): Promise<void> {
  const sorted = sortedCheckpoints(checkpoints);
  const intervalMs = 1000 / context.config.typingCps;
  let checkpointIndex = 0;
  let offset = 0;
  while (offset < finalText.length) {
    let next = Math.min(finalText.length, offset + 3);
    if (checkpointIndex < sorted.length && sorted[checkpointIndex].atLength < next) {
      next = sorted[checkpointIndex].atLength;
    }
    offset = next;
    context.session.changeFile(relativePath, finalText.slice(0, offset));
    await sleep(intervalMs);
    while (checkpointIndex < sorted.length && sorted[checkpointIndex].atLength === offset) {
      const checkpoint = sorted[checkpointIndex];
      checkpointIndex += 1;
      await runCheckpoint(context, checkpoint.makeProbe(finalText.slice(0, offset)), failures);
    }
  }
}

/**
 * Keystroke-level typing of an insertion BEFORE a unique anchor in an
 * existing document (the caret moved mid-document); checkpoints fire at exact
 * insertion lengths with the cursor pinned to the end of the inserted text.
 */
export async function typeInsertion(
  context: ScenarioContext,
  relativePath: string,
  anchor: string,
  inserted: string,
  checkpoints: readonly TypingCheckpoint[],
  failures: FailureBag,
): Promise<void> {
  const sorted = sortedCheckpoints(checkpoints);
  const intervalMs = 1000 / context.config.typingCps;
  let checkpointIndex = 0;
  let offset = 0;
  while (offset < inserted.length) {
    let next = Math.min(inserted.length, offset + 3);
    if (checkpointIndex < sorted.length && sorted[checkpointIndex].atLength < next) {
      next = sorted[checkpointIndex].atLength;
    }
    offset = next;
    const piece = inserted.slice(0, offset);
    const current = context.session.textOf(relativePath);
    context.session.changeFile(relativePath, replaceOnce(current, anchor, piece + anchor));
    await sleep(intervalMs);
    while (checkpointIndex < sorted.length && sorted[checkpointIndex].atLength === offset) {
      const checkpoint = sorted[checkpointIndex];
      checkpointIndex += 1;
      await runCheckpoint(context, checkpoint.makeProbe(piece), failures);
    }
  }
}

/** Assemble the attestation receipt for a finished scenario run. */
export function buildReceipt(
  context: ScenarioContext,
  startedAtMs: number,
  options: {
    finalSanityPass: boolean | null;
    failures: readonly string[];
  },
): EnduranceReceipt {
  const { session, config } = context;
  const degradation = session.recorder.degradation(
    config.degradationFactor,
    config.degradationFloorMs,
  );
  const durationMs = Date.now() - startedAtMs;
  // INFORMATIONAL edit-pipeline measurement: offered didChange rate × the
  // server-measured did_change handler cost → pipeline utilization. Never
  // asserted; the superhuman-rate ceiling is data, not a gate.
  const editsPerSecond = durationMs > 0 ? session.tracker.editsSent / (durationMs / 1000) : 0;
  const handlerCosts = parseHandlerExitCostsMs(session.client.stderr.text(), "did_change");
  const didChangeHandlerMs =
    handlerCosts.length > 0
      ? {
          samples: handlerCosts.length,
          p50: percentileOf(handlerCosts, 50),
          max: Math.max(...handlerCosts),
        }
      : null;
  const pipelineUtilization =
    didChangeHandlerMs !== null ? (editsPerSecond * didChangeHandlerMs.p50) / 1000 : null;
  return {
    schemaVersion: 1,
    scenario: context.scenario,
    route: context.route,
    startedAt: new Date(startedAtMs).toISOString(),
    durationMs,
    requestsSent: session.tracker.sent,
    requestsAnswered: session.tracker.answered,
    requestsCancelled: session.tracker.cancelled,
    requestsErrored: session.tracker.errored,
    requestsUnanswered: session.tracker.unanswered,
    editsSent: session.tracker.editsSent,
    latency: {
      overall: session.recorder.overall(),
      windows: session.recorder.windows(),
    },
    maxRssBytes: context.sampler?.maxRssBytes ?? null,
    rssSupported: context.sampler?.supported ?? false,
    providerAliveAtEnd: context.session.client.isAlive(),
    finalSanityPass: options.finalSanityPass,
    degradationCheck: degradation,
    typeQuality: session.typeQuality.snapshot(),
    config: {
      p95MaxMs: config.p95MaxMs,
      stormP95MaxMs: config.stormP95MaxMs,
      rssMaxBytes: config.rssMaxBytes,
      requestTimeoutMs: config.requestTimeoutMs,
    },
    throughputCeiling: {
      editsPerSecond,
      didChangeHandlerMs,
      pipelineUtilization,
    },
    failures: options.failures,
  };
}
