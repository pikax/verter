/**
 * @ai-generated - Tests for the activation gate used to serialize extension startup.
 */
import { describe, expect, it, vi } from "vitest";
import { createActivationGate } from "./activationGate";

describe("createActivationGate", () => {
  it("shares one in-flight activation across concurrent callers", async () => {
    let resolveStart: (() => void) | undefined;
    const start = vi.fn().mockImplementation(
      () =>
        new Promise<string>((resolve) => {
          resolveStart = () => resolve("ready");
        }),
    );

    const gate = createActivationGate(start);
    const first = gate.run();
    const second = gate.run();

    expect(start).toHaveBeenCalledOnce();
    expect(first).toBe(second);

    resolveStart?.();

    await expect(first).resolves.toBe("ready");
    await expect(second).resolves.toBe("ready");
    expect(gate.isActive()).toBe(true);
  });

  it("retries after a failed activation attempt", async () => {
    const start = vi.fn().mockRejectedValueOnce(new Error("boom")).mockResolvedValueOnce("ready");

    const gate = createActivationGate(start);

    await expect(gate.run()).rejects.toThrow("boom");
    expect(gate.isActive()).toBe(false);

    await expect(gate.run()).resolves.toBe("ready");
    expect(start).toHaveBeenCalledTimes(2);
    expect(gate.isActive()).toBe(true);
  });

  it("allows a fresh activation after reset", async () => {
    const start = vi.fn().mockResolvedValue("ready");
    const gate = createActivationGate(start);

    await gate.run();
    gate.reset();
    await gate.run();

    expect(start).toHaveBeenCalledTimes(2);
  });

  it("a superseded attempt's late rejection does not unlock a third start", async () => {
    const parked: Array<{
      resolve: (value: string) => void;
      reject: (error: Error) => void;
    }> = [];
    const start = vi.fn().mockImplementation(
      () =>
        new Promise<string>((resolve, reject) => {
          parked.push({ resolve, reject });
        }),
    );

    const gate = createActivationGate(start);
    const superseded = gate.run();
    gate.reset();
    const live = gate.run();

    parked[1].resolve("second");
    await expect(live).resolves.toBe("second");

    // The first attempt settles after its replacement completed.
    parked[0].reject(new Error("stale activation failure"));
    await expect(superseded).rejects.toThrow("stale activation failure");

    // The live attempt still owns the gate, so a new caller joins it instead
    // of starting a third activation.
    expect(gate.run()).resolves.toBe("second");
    expect(start).toHaveBeenCalledTimes(2);
  });
});
