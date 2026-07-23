import { afterEach, describe, expect, it, vi } from "vitest";
import { createTypeScriptPluginRefreshScheduler } from "./typescriptPluginRefreshScheduler";

describe("TypeScript plugin refresh scheduler", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("collapses a publication storm and still flushes at the maximum delay", () => {
    vi.useFakeTimers();
    const refresh = vi.fn();
    const scheduler = createTypeScriptPluginRefreshScheduler(refresh, {
      idleDelayMs: 50,
      maximumDelayMs: 200,
    });

    for (let elapsed = 0; elapsed < 200; elapsed += 30) {
      scheduler.request();
      vi.advanceTimersByTime(30);
    }

    expect(refresh).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(100);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("flushes a quiet batch on the trailing edge and starts a fresh batch later", () => {
    vi.useFakeTimers();
    const refresh = vi.fn();
    const scheduler = createTypeScriptPluginRefreshScheduler(refresh, {
      idleDelayMs: 40,
      maximumDelayMs: 200,
    });

    scheduler.request();
    vi.advanceTimersByTime(20);
    scheduler.request();
    vi.advanceTimersByTime(39);
    expect(refresh).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(refresh).toHaveBeenCalledTimes(1);

    scheduler.request();
    vi.advanceTimersByTime(40);
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("cancels pending work when disposed", () => {
    vi.useFakeTimers();
    const refresh = vi.fn();
    const scheduler = createTypeScriptPluginRefreshScheduler(refresh);

    scheduler.request();
    scheduler.dispose();
    vi.runAllTimers();

    expect(refresh).not.toHaveBeenCalled();
  });
});
