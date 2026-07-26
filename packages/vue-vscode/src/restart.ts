/**
 * Extracted language server restart logic for testability.
 *
 * The core restart flow is isolated from VS Code dependencies so it can
 * be unit-tested with mock implementations.
 */

export interface RestartLog {
  info(msg: string): void;
  warn(msg: string, ...args: unknown[]): void;
  error(msg: string, ...args: unknown[]): void;
}

export interface RestartDeps {
  /** Stop the current language server. May throw (e.g., timeout). */
  stop: () => Promise<void>;
  /** Create a new language server client and start it. */
  createAndStart: () => Promise<void>;
  /** Kill the tracked type provider child process (orphan cleanup on stop failure). */
  killTrackedTypeProvider: () => void;
  /** Reset dependent services (CSS service, diagnostics). */
  resetServices: () => void;
  log: RestartLog;
}

/**
 * Restart the language server with graceful stop-failure recovery.
 *
 * If `stop()` fails (e.g., shutdown timeout), the TSGO child process is
 * killed explicitly and a new server is still created. This prevents the
 * client from getting stuck pointing to a dead server instance.
 *
 * @returns `true` if the restart succeeded, `false` if it failed.
 */
export async function restartLanguageServer(deps: RestartDeps): Promise<boolean> {
  try {
    deps.log.info("Restarting language server...");
    try {
      await deps.stop();
    } catch (e) {
      deps.log.warn("Failed to stop language server cleanly, forcing restart", e);
      deps.killTrackedTypeProvider();
    }
    await deps.createAndStart();
    deps.resetServices();
    return true;
  } catch (e) {
    deps.log.error("Failed to restart language server", e);
    return false;
  }
}

/** How hard, and how often, automatic recovery is allowed to try. */
export interface RestartPolicy {
  /** Consecutive automatic restarts allowed before recovery gives up for good. */
  maxAutomaticRestarts: number;
  /** Delay before the first automatic attempt. */
  initialBackoffMs: number;
  /** Multiplier applied to the delay on each further consecutive attempt. */
  backoffFactor: number;
  /** Ceiling for a single delay, so backoff never grows without bound. */
  maxBackoffMs: number;
  /**
   * How long a restarted server must run WITHOUT a further automatic restart
   * request before the consecutive count is cleared.
   *
   * This window — not "start() returned" — is what clears the count, and the
   * distinction is the whole point. A server that starts cleanly and dies two
   * seconds later would reset a start-based counter on every cycle, so such a
   * counter can never trip and recovery runs forever. Requiring the server to
   * stay up makes a crash loop converge on the terminal give-up instead.
   */
  healthyResetMs: number;
}

export const DEFAULT_RESTART_POLICY: RestartPolicy = {
  maxAutomaticRestarts: 5,
  initialBackoffMs: 2_000,
  backoffFactor: 2,
  maxBackoffMs: 60_000,
  healthyResetMs: 300_000,
};

/**
 * What a restart request did.
 *
 * - `restarted` — a restart ran and the server came back.
 * - `failed` — a restart ran and the server did not come back (manual requests
 *   only; an automatic request keeps retrying inside its budget).
 * - `suppressed` — no restart ran: recovery has given up, or the supervisor
 *   was disposed.
 * - `in-progress` — no restart ran: one is already running.
 */
export type RestartOutcome = "restarted" | "failed" | "suppressed" | "in-progress";

/** The two timers the supervisor owns, injectable so tests own elapsed time. */
export interface RestartTimers {
  /** Resolve after `ms` have elapsed. */
  sleep(ms: number): Promise<void>;
  /** Run `fn` after `ms`; the returned function cancels it. */
  schedule(ms: number, fn: () => void): () => void;
}

const REAL_TIMERS: RestartTimers = {
  sleep: (ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
  schedule: (ms, fn) => {
    const handle = setTimeout(fn, ms);
    return () => clearTimeout(handle);
  },
};

export interface RestartSupervisorOptions {
  /** Perform one restart. Resolves `true` when the server is running again. */
  restart: () => Promise<boolean>;
  /** Terminal notification: recovery stopped, and here is how to retry by hand. */
  onGiveUp: (message: string) => void;
  log: RestartLog;
  policy?: Partial<RestartPolicy>;
  timers?: RestartTimers;
}

export interface RestartSupervisor {
  /**
   * Recover from a detected freeze or crash. Bounded: delayed by backoff,
   * capped at `maxAutomaticRestarts` consecutive attempts, and terminal
   * afterwards until a manual restart re-arms it.
   */
  requestAutomaticRestart(reason: string): Promise<RestartOutcome>;
  /** A user-initiated restart: always attempted, and it clears the terminal state. */
  requestManualRestart(): Promise<RestartOutcome>;
  /** True once automatic recovery has been permanently suppressed. */
  hasGivenUp(): boolean;
  /** Automatic restarts since the last sustained-health window. */
  consecutiveAutomaticRestarts(): number;
  /**
   * Shut recovery down for good: abandon a backoff that is still counting down,
   * cancel the sustained-health timer, and refuse every later request.
   *
   * This is deactivation. The extension instance that owned the server is gone,
   * so a restart completing afterwards would spawn a process no instance owns
   * and nothing would ever shut down. A restart already executing cannot be
   * recalled, but nothing is armed on its result and no further attempt follows
   * it.
   */
  dispose(): void;
}

/**
 * Bounded, backed-off supervision of language-server restarts.
 *
 * Recovery owns its own retry loop rather than leaning on the caller's watchdog
 * re-arming itself: a start that THROWS leaves no client to send heartbeats, so
 * a watchdog-driven scheme either stops retrying entirely or (as it did) re-arms
 * a timer against a server that was never running. One request therefore drives
 * attempts until the server comes back or the budget is spent.
 */
export function createRestartSupervisor(options: RestartSupervisorOptions): RestartSupervisor {
  const policy: RestartPolicy = { ...DEFAULT_RESTART_POLICY, ...options.policy };
  const timers = options.timers ?? REAL_TIMERS;
  const { log } = options;

  let attempts = 0;
  let running = false;
  let gaveUp = false;
  let disposed = false;
  let cancelHealthReset: (() => void) | undefined;

  // Resolves on disposal, so a recovery parked in its backoff stops waiting the
  // delay out and observes the shutdown at once instead of at the far end of it.
  let releaseBackoff: () => void = () => {};
  const disposedSignal = new Promise<void>((resolve) => {
    releaseBackoff = resolve;
  });

  function clearHealthReset() {
    cancelHealthReset?.();
    cancelHealthReset = undefined;
  }

  function dispose() {
    if (disposed) {
      return;
    }
    disposed = true;
    clearHealthReset();
    releaseBackoff();
  }

  function armHealthReset() {
    clearHealthReset();
    if (disposed) {
      // A window armed here would outlive the extension and fire against state
      // nobody owns any more.
      return;
    }
    cancelHealthReset = timers.schedule(policy.healthyResetMs, () => {
      cancelHealthReset = undefined;
      if (attempts === 0) {
        return;
      }
      log.info(
        `Verter language server has been stable for ${policy.healthyResetMs / 1000}s — automatic restart budget restored.`,
      );
      attempts = 0;
    });
  }

  function backoffFor(attempt: number): number {
    const raw = policy.initialBackoffMs * policy.backoffFactor ** (attempt - 1);
    return Math.min(raw, policy.maxBackoffMs);
  }

  function giveUp(): void {
    gaveUp = true;
    clearHealthReset();
    const message =
      `Verter language server did not stay running after ${policy.maxAutomaticRestarts} automatic restarts. ` +
      `Automatic restarts are now disabled — run the "Verter: Restart Language Server" command to try again, ` +
      `and check the Verter output channel for the underlying failure.`;
    log.error(message);
    options.onGiveUp(message);
  }

  async function requestAutomaticRestart(reason: string): Promise<RestartOutcome> {
    if (disposed) {
      return "suppressed";
    }
    if (gaveUp) {
      log.warn(`Ignoring automatic restart request (${reason}): automatic restarts are disabled.`);
      return "suppressed";
    }
    if (running) {
      return "in-progress";
    }
    running = true;
    try {
      for (;;) {
        if (attempts >= policy.maxAutomaticRestarts) {
          giveUp();
          return "suppressed";
        }
        attempts += 1;
        const delay = backoffFor(attempts);
        log.warn(
          `Automatic language server restart ${attempts}/${policy.maxAutomaticRestarts} in ${delay / 1000}s (${reason}).`,
        );
        clearHealthReset();
        await Promise.race([timers.sleep(delay), disposedSignal]);
        // The delay is the window this fails in: the extension can be disposed
        // while recovery sits in it, and a restart afterwards spawns a server
        // for an extension instance that no longer exists.
        if (disposed) {
          return "suppressed";
        }
        if (await options.restart()) {
          armHealthReset();
          return "restarted";
        }
        if (disposed) {
          return "suppressed";
        }
      }
    } finally {
      running = false;
    }
  }

  async function requestManualRestart(): Promise<RestartOutcome> {
    if (disposed) {
      return "suppressed";
    }
    if (running) {
      return "in-progress";
    }
    running = true;
    try {
      // The user asked for this one, so it runs now: no backoff, and the
      // terminal state clears whether or not this attempt succeeds.
      gaveUp = false;
      attempts = 0;
      clearHealthReset();
      return (await options.restart()) ? "restarted" : "failed";
    } finally {
      running = false;
    }
  }

  return {
    requestAutomaticRestart,
    requestManualRestart,
    hasGivenUp: () => gaveUp,
    consecutiveAutomaticRestarts: () => attempts,
    dispose,
  };
}
