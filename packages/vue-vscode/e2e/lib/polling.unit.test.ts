import { afterEach, describe, expect, it, vi } from "vitest";
import { pollUntilWithin } from "./polling";

describe("pollUntilWithin", () => {
  afterEach(() => vi.useRealTimers());

  it("accepts a ready observation that completes after the poll deadline", async () => {
    vi.useFakeTimers();
    const result = pollUntilWithin(
      "slow ready provider",
      async () => {
        await new Promise((resolve) => setTimeout(resolve, 20));
        return "ready";
      },
      (value) => value === "ready",
      10,
      1,
    );

    await vi.advanceTimersByTimeAsync(20);
    await expect(result).resolves.toBe("ready");
  });

  it("rejects an unready observation that completes after the poll deadline", async () => {
    vi.useFakeTimers();
    const result = pollUntilWithin(
      "slow empty provider",
      async () => {
        await new Promise((resolve) => setTimeout(resolve, 20));
        return "empty";
      },
      (value) => value === "ready",
      10,
      1,
    );
    const rejected = expect(result).rejects.toThrow("slow empty provider not ready within 10ms");

    await vi.advanceTimersByTimeAsync(20);
    await rejected;
  });
});
