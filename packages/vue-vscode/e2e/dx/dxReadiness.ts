/**
 * In-host startup-readiness gate for the extension-host DX driver.
 *
 * `$/verter/ready` is non-semantic: it is published on the main init path while
 * `$/verter/typeProviderSyncComplete` is published from a separately spawned task
 * gated on the workspace scanner, so the two RACE and either may arrive first.
 * Cross-file type resolution is only reliable once BOTH have arrived for the SAME
 * (necessarily newest) generation; a re-init bumps the generation and supersedes
 * the old one. The in-host `waitForTypeProviderSync` helper accepts `sync >= ready`
 * and is therefore too weak for DX — this gate requires the matching-generation
 * newest pair and discards superseded generations.
 *
 * There is exactly ONE startup-gate engine: the harness's `parseExtensionStartupLog`
 * + matching-generation fold (`@verter/dx-harness/startup-gate`). This module does
 * NOT re-implement that fold — it takes it as an injected `evaluateLog` callback so
 * the live wait stays unit-testable, and the in-host wiring loads the real harness
 * parser at runtime (see `dxScenarioRunner.ts`). The wait loop re-reads the FULL
 * accumulated log and re-runs that fold on EVERY poll, so a generation that is
 * superseded mid-quiescence (e.g. a late `ready(2)`) re-arms the gate instead of
 * resolving stale.
 *
 * `waitForDxReadiness` takes all its I/O (and the fold) as injected callbacks, so it
 * is testable without VS Code.
 */

/**
 * The slice of a startup-gate verdict the wait loop consumes. Structurally a subset
 * of the harness `GenerationGateDecision`, so the harness `parseExtensionStartupLog`
 * is assignable to {@link DxReadinessOptions.evaluateLog} directly.
 */
export interface StartupGateDecision {
  /** Both channels have reached the same (newest) generation. */
  readonly satisfied: boolean;
  /** The matched generation when {@link satisfied}, else `null`. */
  readonly matchedGeneration: number | null;
  /** Highest generation seen on the `ready` channel (`null` if none). */
  readonly maxReadyGeneration: number | null;
  /** Highest generation seen on the `sync` channel (`null` if none). */
  readonly maxSyncGeneration: number | null;
}

/** Evaluates the matching-generation gate over log lines (the harness fold). */
export type EvaluateStartupLog = (lines: Iterable<string>) => StartupGateDecision;

/** A single quiescence observation sampled while the gate is satisfied. */
export interface QuiescenceSample {
  readonly diagnosticsCount: number;
  readonly logLength: number;
}

/** Injected I/O + thresholds for {@link waitForDxReadiness}. */
export interface DxReadinessOptions {
  /** Reads the current extension log file contents. */
  readLog: () => string;
  /** The matching-generation fold — the real harness `parseExtensionStartupLog`. */
  evaluateLog: EvaluateStartupLog;
  /** Samples diagnostics/log volume for the quiescence check. */
  sampleQuiescence: () => QuiescenceSample;
  /** Sleeps `ms` between polls. */
  sleep: (ms: number) => Promise<void>;
  /** Monotonic clock in ms. */
  now: () => number;
  /** Overall timeout for the combined gate + quiescence wait. */
  timeoutMs?: number;
  /** Poll interval. */
  intervalMs?: number;
  /** Consecutive identical quiescence samples (while satisfied) to declare quiescence. */
  requiredStableSamples?: number;
}

function splitLines(text: string): string[] {
  return text.split(/\r?\n/);
}

/**
 * Block until the matching-generation gate is satisfied for the NEWEST generation
 * AND diagnostics/log volume has quiesced, then resolve with the matched generation.
 *
 * The full log is re-read and re-folded on every poll — the match is never cached
 * across polls. Quiescence only accrues while the gate is satisfied; if a later
 * `ready(N+1)` supersedes a matched pair at `N`, `satisfied` flips false, the
 * quiescence run resets, and the loop waits for `sync(N+1)` before it can resolve.
 * So it resolves on generation `N+1` and never on the stale `N`. Throws on timeout.
 */
export async function waitForDxReadiness(
  opts: DxReadinessOptions,
): Promise<{ matchedGeneration: number }> {
  const timeoutMs = opts.timeoutMs ?? 60_000;
  const intervalMs = opts.intervalMs ?? 200;
  const requiredStableSamples = Math.max(2, opts.requiredStableSamples ?? 3);

  const start = opts.now();
  let prev: QuiescenceSample | null = null;
  let stableRun = 0;
  let lastDecision: StartupGateDecision = {
    satisfied: false,
    matchedGeneration: null,
    maxReadyGeneration: null,
    maxSyncGeneration: null,
  };

  for (;;) {
    // Re-read the FULL accumulated log and re-run the matching-generation fold every
    // poll — never resolve on a generation that a later line has superseded.
    const decision = opts.evaluateLog(splitLines(opts.readLog()));
    lastDecision = decision;

    if (decision.satisfied && decision.matchedGeneration !== null) {
      const sample = opts.sampleQuiescence();
      const same =
        prev !== null &&
        sample.diagnosticsCount === prev.diagnosticsCount &&
        sample.logLength === prev.logLength;
      stableRun = same ? stableRun + 1 : 1;
      prev = sample;
      if (stableRun >= requiredStableSamples) {
        return { matchedGeneration: decision.matchedGeneration };
      }
    } else {
      // Gate not (no longer) satisfied for the newest generation — re-arm: discard
      // any quiescence progress until both channels match the newest generation again.
      stableRun = 0;
      prev = null;
    }

    if (opts.now() - start >= timeoutMs) {
      throw new Error(
        `DX readiness timed out after ${timeoutMs}ms (satisfied=${lastDecision.satisfied}, ` +
          `ready=${lastDecision.maxReadyGeneration}, sync=${lastDecision.maxSyncGeneration}, ` +
          `stableRun=${stableRun}/${requiredStableSamples})`,
      );
    }
    await opts.sleep(intervalMs);
  }
}
