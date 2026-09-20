/**
 * ARH7-AC2: reload or repeated activation must not duplicate registrations.
 *
 * The composition root owns one ActivationSession. Concurrent activate()
 * shares that session; deactivate() disposes it; a later activate() starts
 * a new session whose registrations replace, rather than stack on, the
 * previous ones.
 */
import { describe, expect, it, vi } from "vitest";
import { createActivationRoot } from "./activationSession";

describe("createActivationRoot", () => {
  it("shares one in-flight activation across concurrent callers", async () => {
    let resolveStart: (() => void) | undefined;
    const start = vi.fn().mockImplementation(
      (session: { scope: { add(...items: { dispose(): void }[]): void } }) =>
        new Promise<string>((resolve) => {
          session.scope.add({ dispose: () => {} });
          resolveStart = () => resolve("ready");
        }),
    );

    const root = createActivationRoot(start);
    root.ensureSession();
    const first = root.run();
    const second = root.run();

    expect(start).toHaveBeenCalledOnce();
    expect(first).toBe(second);

    resolveStart?.();
    await expect(first).resolves.toBe("ready");
    await expect(second).resolves.toBe("ready");
    expect(root.getRuntime()).toBe("ready");
  });

  it("reload or repeated activation does not duplicate registrations", async () => {
    const disposed: string[] = [];
    let generation = 0;
    const root = createActivationRoot(async (session) => {
      generation += 1;
      const id = `gen-${generation}`;
      session.scope.add({
        dispose: () => {
          disposed.push(id);
        },
      });
      return id;
    });

    root.ensureSession();
    await root.run();
    root.ensureSession();
    await root.run();
    expect(generation).toBe(1);
    expect(disposed).toEqual([]);

    root.deactivate();
    expect(disposed).toEqual(["gen-1"]);

    root.ensureSession();
    await root.run();
    expect(generation).toBe(2);
    expect(disposed).toEqual(["gen-1"]);

    root.deactivate();
    expect(disposed).toEqual(["gen-1", "gen-2"]);
  });

  it("retries after a failed activation on a fresh session", async () => {
    const disposed: string[] = [];
    let attempts = 0;
    const root = createActivationRoot(async (session) => {
      attempts += 1;
      const id = `attempt-${attempts}`;
      session.scope.add({
        dispose: () => {
          disposed.push(id);
        },
      });
      if (attempts === 1) {
        throw new Error("boom");
      }
      return id;
    });

    root.ensureSession();
    await expect(root.run()).rejects.toThrow("boom");
    expect(disposed).toEqual(["attempt-1"]);
    expect(root.getSession()).toBeUndefined();

    root.ensureSession();
    await expect(root.run()).resolves.toBe("attempt-2");
    expect(disposed).toEqual(["attempt-1"]);

    root.deactivate();
    expect(disposed).toEqual(["attempt-1", "attempt-2"]);
  });

  it("deactivate is idempotent and clears runtime handles", async () => {
    const root = createActivationRoot(async () => "ready");
    root.ensureSession();
    await root.run();
    expect(root.getRuntime()).toBe("ready");

    root.deactivate();
    root.deactivate();
    expect(root.getRuntime()).toBeUndefined();
    expect(root.getSession()).toBeUndefined();
  });

  it("a stale activation's rejection cannot dispose or replace the live session", async () => {
    const disposed: string[] = [];
    let rejectA: ((error: Error) => void) | undefined;
    let generation = 0;
    const root = createActivationRoot(
      (session) =>
        new Promise<string>((resolve, reject) => {
          generation += 1;
          const id = `gen-${generation}`;
          session.scope.add({ dispose: () => disposed.push(id) });
          if (generation === 1) {
            rejectA = reject;
          } else {
            resolve(id);
          }
        }),
    );

    // Park activation A, deactivate, then let activation B complete.
    root.ensureSession();
    const staleRun = root.run();
    root.deactivate();
    expect(disposed).toEqual(["gen-1"]);

    root.ensureSession();
    await expect(root.run()).resolves.toBe("gen-2");
    const liveSession = root.getSession();
    expect(liveSession?.isDisposed).toBe(false);
    expect(root.getRuntime()).toBe("gen-2");

    // A settles last, as a rejection. The replacement must survive it.
    rejectA?.(new Error("stale activation failure"));
    await expect(staleRun).rejects.toThrow("stale activation failure");
    expect(root.getSession()).toBe(liveSession);
    expect(root.getSession()?.isDisposed).toBe(false);
    expect(root.getRuntime()).toBe("gen-2");
    expect(disposed).toEqual(["gen-1"]);

    // Single-start: a later caller still joins the live activation rather
    // than starting a third one over it.
    await expect(root.run()).resolves.toBe("gen-2");
    expect(generation).toBe(2);
  });

  it("a stale activation's success cannot republish its runtime", async () => {
    let resolveA: ((value: string) => void) | undefined;
    let generation = 0;
    const root = createActivationRoot(
      (session) =>
        new Promise<string>((resolve) => {
          generation += 1;
          session.scope.add({ dispose: () => {} });
          if (generation === 1) {
            resolveA = resolve;
          } else {
            resolve(`gen-${generation}`);
          }
        }),
    );

    root.ensureSession();
    const staleRun = root.run();
    root.deactivate();

    root.ensureSession();
    await expect(root.run()).resolves.toBe("gen-2");
    expect(root.getRuntime()).toBe("gen-2");

    resolveA?.("gen-1");
    await expect(staleRun).resolves.toBe("gen-1");
    // The stale generation resolved, but only the live session's runtime is
    // published by the root.
    expect(root.getRuntime()).toBe("gen-2");
    expect(root.getSession()?.isDisposed).toBe(false);
  });

  it("a stale activation settles harmlessly when no replacement exists", async () => {
    const parked: Array<{
      resolve: (value: string) => void;
      reject: (error: Error) => void;
    }> = [];
    let generation = 0;
    const root = createActivationRoot(
      (session) =>
        new Promise<string>((resolve, reject) => {
          generation += 1;
          session.scope.add({ dispose: () => {} });
          parked.push({ resolve, reject });
        }),
    );

    // A rejects after deactivation with no replacement: the root is empty,
    // so the cleanup must not reach for a session that no longer exists.
    root.ensureSession();
    const rejectedRun = root.run();
    root.deactivate();
    parked[0].reject(new Error("stale activation failure"));
    await expect(rejectedRun).rejects.toThrow("stale activation failure");
    expect(root.getRuntime()).toBeUndefined();
    expect(root.getSession()).toBeUndefined();

    // A late success is equally inert: nothing is republished into the
    // deactivated root.
    root.ensureSession();
    const staleRun = root.run();
    root.deactivate();
    parked[1].resolve(`gen-${generation}`);
    await expect(staleRun).resolves.toBe(`gen-${generation}`);
    expect(root.getRuntime()).toBeUndefined();
    expect(root.getSession()).toBeUndefined();
  });
});
