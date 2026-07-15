import { describe, expect, it, vi } from "vitest";

import { raceWithTimeout } from "./query-timeout.js";

describe("raceWithTimeout", () => {
  it("clears the timeout handle when the wrapped promise resolves first", async () => {
    vi.useFakeTimers();

    try {
      const resultPromise = raceWithTimeout(Promise.resolve("ok"), 14_000, "timeout");
      await Promise.resolve();

      expect(await resultPromise).toBe("ok");
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("returns the fallback value when the timeout wins", async () => {
    vi.useFakeTimers();

    try {
      const resultPromise = raceWithTimeout(new Promise<string>(() => {}), 14_000, "timeout");

      await vi.advanceTimersByTimeAsync(14_000);

      expect(await resultPromise).toBe("timeout");
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
