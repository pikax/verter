/**
 * Tests for the language server restart logic.
 *
 * These tests verify the restart flow handles all failure scenarios,
 * especially the critical case where `stop()` times out.
 */
import { describe, it, expect, vi } from "vitest";
import { restartLanguageServer, type RestartDeps } from "./restart";

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
      createAndStart: vi
        .fn()
        .mockRejectedValue(new Error("Failed to start")),
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
      createAndStart: vi
        .fn()
        .mockRejectedValue(new Error("Failed to start")),
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
