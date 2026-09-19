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
});
