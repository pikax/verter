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
    const start = vi
      .fn()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce("ready");

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
});
