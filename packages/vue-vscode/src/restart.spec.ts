/**
 * Tests for the language server restart logic.
 *
 * These tests verify the restart flow handles all failure scenarios,
 * especially the critical case where `stop()` times out.
 */
import { describe, it, expect, vi } from "vitest";
import {
  createRestartSupervisor,
  DEFAULT_RESTART_POLICY,
  restartLanguageServer,
  type RestartDeps,
  type RestartTimers,
} from "./restart";

function makeDeps(overrides: Partial<RestartDeps> = {}): RestartDeps {
  return {
    stop: vi.fn().mockResolvedValue(undefined),
    createAndStart: vi.fn().mockResolvedValue(undefined),
    killTrackedTypeProvider: vi.fn(),
    resetServices: vi.fn(),
    log: {
      info: vi.fn(),
      warn: vi.fn(),
      error: vi.fn(),
    },
    ...overrides,
  };
}

describe("restartLanguageServer", () => {
  it("happy path: stop → create → reset", async () => {
    const deps = makeDeps();
    const result = await restartLanguageServer(deps);

    expect(result).toBe(true);
    expect(deps.stop).toHaveBeenCalledOnce();
    expect(deps.createAndStart).toHaveBeenCalledOnce();
    expect(deps.resetServices).toHaveBeenCalledOnce();
    expect(deps.killTrackedTypeProvider).not.toHaveBeenCalled();
  });

  it("stop timeout recovery: creates new server even when stop throws", async () => {
    const deps = makeDeps({
      stop: vi.fn().mockRejectedValue(new Error("Stopping server timed out")),
    });

    const result = await restartLanguageServer(deps);

    expect(result).toBe(true);
    // Critical: new server must still be created
    expect(deps.createAndStart).toHaveBeenCalledOnce();
    expect(deps.resetServices).toHaveBeenCalledOnce();
    // Type provider orphan killed on stop failure
    expect(deps.killTrackedTypeProvider).toHaveBeenCalledOnce();
    expect(deps.log.warn).toHaveBeenCalledWith(
      "Failed to stop language server cleanly, forcing restart",
      expect.any(Error),
    );
  });

  it("start failure: error logged, returns false", async () => {
    const deps = makeDeps({
      createAndStart: vi.fn().mockRejectedValue(new Error("Failed to start")),
    });

    const result = await restartLanguageServer(deps);

    expect(result).toBe(false);
    expect(deps.log.error).toHaveBeenCalledWith(
      "Failed to restart language server",
      expect.any(Error),
    );
    // Services should NOT be reset since start failed
    expect(deps.resetServices).not.toHaveBeenCalled();
  });

  it("stop timeout + start failure: TSGO killed, error logged", async () => {
    const deps = makeDeps({
      stop: vi.fn().mockRejectedValue(new Error("timeout")),
      createAndStart: vi.fn().mockRejectedValue(new Error("Failed to start")),
    });

    const result = await restartLanguageServer(deps);

    expect(result).toBe(false);
    expect(deps.killTrackedTypeProvider).toHaveBeenCalledOnce();
    expect(deps.log.error).toHaveBeenCalled();
  });

  it("services are reset only after successful start", async () => {
    const callOrder: string[] = [];
    const deps = makeDeps({
      createAndStart: vi.fn().mockImplementation(async () => {
        callOrder.push("createAndStart");
      }),
      resetServices: vi.fn().mockImplementation(() => {
        callOrder.push("resetServices");
      }),
    });

    await restartLanguageServer(deps);

    expect(callOrder).toEqual(["createAndStart", "resetServices"]);
  });
});

/**
 * A controllable stand-in for the two timers the supervisor owns: the backoff
 * sleep between attempts, and the sustained-health window that clears the
 * consecutive-failure count. Nothing here elapses on its own — the test decides.
 */
function makeTimers() {
  const slept: number[] = [];
  const scheduled: { ms: number; fn: () => void; cancelled: boolean }[] = [];
  const timers: RestartTimers = {
    sleep: async (ms) => {
      slept.push(ms);
    },
    schedule: (ms, fn) => {
      const entry = { ms, fn, cancelled: false };
      scheduled.push(entry);
      return () => {
        entry.cancelled = true;
      };
    },
  };
  return {
    timers,
    slept,
    scheduled,
    /** Fire every live scheduled callback, as real elapsed time would. */
    elapse() {
      for (const entry of scheduled.splice(0)) {
        if (!entry.cancelled) entry.fn();
      }
    },
  };
}

function makeSupervisor(
  restart: () => Promise<boolean>,
  overrides: { policy?: Partial<typeof DEFAULT_RESTART_POLICY> } = {},
) {
  const clock = makeTimers();
  const onGiveUp = vi.fn();
  const log = { info: vi.fn(), warn: vi.fn(), error: vi.fn() };
  const supervisor = createRestartSupervisor({
    restart,
    onGiveUp,
    log,
    timers: clock.timers,
    policy: overrides.policy,
  });
  return { supervisor, clock, onGiveUp, log };
}

describe("createRestartSupervisor", () => {
  it("stops automatic restarts at the cap and reports a terminal give-up", async () => {
    const restart = vi.fn().mockResolvedValue(false);
    const { supervisor, onGiveUp } = makeSupervisor(restart);

    // One watchdog trip against a server that can never start. The supervisor
    // owns the retry loop, so this single request exhausts the whole budget.
    const first = await supervisor.requestAutomaticRestart("no heartbeat");
    expect(first).toBe("suppressed");
    expect(restart).toHaveBeenCalledTimes(DEFAULT_RESTART_POLICY.maxAutomaticRestarts);
    expect(supervisor.hasGivenUp()).toBe(true);
    expect(onGiveUp).toHaveBeenCalledTimes(1);
    expect(onGiveUp.mock.calls[0][0]).toMatch(/Restart Language Server/);

    // Every later watchdog trip is refused outright — no further process spawn.
    for (let i = 0; i < 5; i += 1) {
      expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("suppressed");
    }
    expect(restart).toHaveBeenCalledTimes(DEFAULT_RESTART_POLICY.maxAutomaticRestarts);
    expect(onGiveUp).toHaveBeenCalledTimes(1);
  });

  it("backs off between attempts, growing geometrically up to the ceiling", async () => {
    const restart = vi.fn().mockResolvedValue(false);
    const { supervisor, clock } = makeSupervisor(restart, {
      policy: {
        maxAutomaticRestarts: 6,
        initialBackoffMs: 1_000,
        backoffFactor: 2,
        maxBackoffMs: 8_000,
      },
    });

    await supervisor.requestAutomaticRestart("no heartbeat");

    expect(clock.slept).toEqual([1_000, 2_000, 4_000, 8_000, 8_000, 8_000]);
  });

  it("still restarts a healthy server, and does not give up", async () => {
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, onGiveUp } = makeSupervisor(restart);

    expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("restarted");
    expect(restart).toHaveBeenCalledTimes(1);
    expect(supervisor.hasGivenUp()).toBe(false);
    expect(onGiveUp).not.toHaveBeenCalled();
  });

  it("a server that starts and immediately dies still trips the cap", async () => {
    // THE discriminator for the counter-reset policy. `restart()` resolves TRUE
    // every time — the server does start — but it never survives long enough to
    // clear the sustained-health window, so the count must NOT reset.
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, onGiveUp } = makeSupervisor(restart);

    const max = DEFAULT_RESTART_POLICY.maxAutomaticRestarts;
    for (let i = 0; i < max; i += 1) {
      expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("restarted");
    }
    expect(supervisor.consecutiveAutomaticRestarts()).toBe(max);

    expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("suppressed");
    expect(restart).toHaveBeenCalledTimes(max);
    expect(onGiveUp).toHaveBeenCalledTimes(1);
  });

  it("clears the count only after the sustained-health window elapses", async () => {
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, clock, onGiveUp } = makeSupervisor(restart);

    const max = DEFAULT_RESTART_POLICY.maxAutomaticRestarts;
    for (let i = 0; i < max - 1; i += 1) {
      await supervisor.requestAutomaticRestart("no heartbeat");
    }
    expect(supervisor.consecutiveAutomaticRestarts()).toBe(max - 1);

    // The server stayed up for the whole health window: the budget is restored.
    clock.elapse();
    expect(supervisor.consecutiveAutomaticRestarts()).toBe(0);

    for (let i = 0; i < max; i += 1) {
      expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("restarted");
    }
    expect(onGiveUp).not.toHaveBeenCalled();
    expect(restart).toHaveBeenCalledTimes(max - 1 + max);
  });

  it("a manual restart clears the terminal state and the count", async () => {
    let succeed = false;
    const restart = vi.fn().mockImplementation(async () => succeed);
    const { supervisor, onGiveUp } = makeSupervisor(restart);

    await supervisor.requestAutomaticRestart("no heartbeat");
    expect(supervisor.hasGivenUp()).toBe(true);

    succeed = true;
    expect(await supervisor.requestManualRestart()).toBe("restarted");
    expect(supervisor.hasGivenUp()).toBe(false);
    expect(supervisor.consecutiveAutomaticRestarts()).toBe(0);

    // Automatic recovery is armed again after the user's manual restart.
    expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("restarted");
    expect(onGiveUp).toHaveBeenCalledTimes(1);
  });

  it("a manual restart runs immediately, without a backoff sleep", async () => {
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, clock } = makeSupervisor(restart);

    expect(await supervisor.requestManualRestart()).toBe("restarted");
    expect(clock.slept).toEqual([]);
  });

  it("refuses a concurrent restart instead of stacking a second one", async () => {
    let release: (value: boolean) => void = () => {};
    const restart = vi.fn().mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          release = resolve;
        }),
    );
    const { supervisor } = makeSupervisor(restart);

    const inFlight = supervisor.requestAutomaticRestart("no heartbeat");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(await supervisor.requestManualRestart()).toBe("in-progress");
    expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("in-progress");
    expect(restart).toHaveBeenCalledTimes(1);

    release(true);
    expect(await inFlight).toBe("restarted");
  });
});

/** Let the microtask queue drain so an in-flight request reaches its next await. */
function tick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * Timers whose backoff sleep completes only when the test releases it.
 *
 * The default fake sleep resolves immediately, which cannot express the state
 * this suite is about: a recovery parked in its backoff while the extension is
 * torn down underneath it.
 */
function makeHeldTimers() {
  const pendingSleeps: (() => void)[] = [];
  const scheduled: { ms: number; fn: () => void; cancelled: boolean }[] = [];
  const timers: RestartTimers = {
    sleep: () =>
      new Promise<void>((resolve) => {
        pendingSleeps.push(resolve);
      }),
    schedule: (ms, fn) => {
      const entry = { ms, fn, cancelled: false };
      scheduled.push(entry);
      return () => {
        entry.cancelled = true;
      };
    },
  };
  return {
    timers,
    scheduled,
    /** The backoff delay elapses, exactly as the real `setTimeout` eventually would. */
    releaseSleeps() {
      for (const resolve of pendingSleeps.splice(0)) resolve();
    },
    /** Fire every live scheduled callback, as real elapsed time would. */
    elapse() {
      for (const entry of scheduled.splice(0)) {
        if (!entry.cancelled) entry.fn();
      }
    },
  };
}

function makeHeldSupervisor(restart: () => Promise<boolean>) {
  const clock = makeHeldTimers();
  const onGiveUp = vi.fn();
  const log = { info: vi.fn(), warn: vi.fn(), error: vi.fn() };
  const supervisor = createRestartSupervisor({
    restart,
    onGiveUp,
    log,
    timers: clock.timers,
  });
  return { supervisor, clock, onGiveUp, log };
}

/**
 * Disposal is deactivation: the extension instance that owned the server is
 * gone. Anything the supervisor still has parked must not start a process,
 * because no extension instance would own it or ever shut it down.
 */
describe("createRestartSupervisor disposal", () => {
  it("never restarts when disposal lands during a pending backoff", async () => {
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, clock } = makeHeldSupervisor(restart);

    const inFlight = supervisor.requestAutomaticRestart("no heartbeat");
    await tick();
    expect(restart).not.toHaveBeenCalled(); // parked in the backoff

    // The user disables the extension / reloads the window mid-backoff.
    supervisor.dispose();
    // The backoff delay expires afterwards, as it does in the real host.
    clock.releaseSleeps();

    expect(await inFlight).toBe("suppressed");
    expect(restart).not.toHaveBeenCalled();
  });

  it("still restarts through that same pending backoff when nothing disposed it", async () => {
    // The control for the test above: the fix must be "no restart after
    // disposal", not "no restart".
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor, clock } = makeHeldSupervisor(restart);

    const inFlight = supervisor.requestAutomaticRestart("no heartbeat");
    await tick();
    expect(restart).not.toHaveBeenCalled();

    clock.releaseSleeps();

    expect(await inFlight).toBe("restarted");
    expect(restart).toHaveBeenCalledTimes(1);
  });

  it("refuses every later restart request once disposed", async () => {
    const restart = vi.fn().mockResolvedValue(true);
    const { supervisor } = makeSupervisor(restart);

    supervisor.dispose();

    expect(await supervisor.requestAutomaticRestart("no heartbeat")).toBe("suppressed");
    expect(await supervisor.requestManualRestart()).toBe("suppressed");
    expect(restart).not.toHaveBeenCalled();
  });

  it("stops the retry loop when disposal lands during an in-flight restart", async () => {
    let releaseFirst: ((started: boolean) => void) | undefined;
    const restart = vi.fn().mockImplementation(() => {
      if (releaseFirst) return Promise.resolve(false);
      return new Promise<boolean>((resolve) => {
        releaseFirst = resolve;
      });
    });
    const { supervisor, onGiveUp } = makeSupervisor(restart);

    const inFlight = supervisor.requestAutomaticRestart("no heartbeat");
    await tick();
    expect(restart).toHaveBeenCalledTimes(1);

    supervisor.dispose();
    releaseFirst?.(false); // that attempt did not bring the server back

    await inFlight;
    // No further attempt, and no error dialog raised at a user whose extension
    // is already gone.
    expect(restart).toHaveBeenCalledTimes(1);
    expect(onGiveUp).not.toHaveBeenCalled();
  });

  it("arms no health-reset window for a restart that completes after disposal", async () => {
    let releaseFirst: ((started: boolean) => void) | undefined;
    const restart = vi.fn().mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          releaseFirst = resolve;
        }),
    );
    const { supervisor, clock, log } = makeHeldSupervisor(restart);

    const inFlight = supervisor.requestAutomaticRestart("no heartbeat");
    await tick();
    clock.releaseSleeps();
    await tick();
    expect(restart).toHaveBeenCalledTimes(1);

    supervisor.dispose();
    releaseFirst?.(true);
    await inFlight;

    // Whatever time passes after deactivation, nothing of the supervisor's runs.
    clock.elapse();
    expect(log.info).not.toHaveBeenCalled();
  });
});
