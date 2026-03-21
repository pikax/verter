/**
 * @ai-generated - Verifies compat checker disposal semantics for host-backed usage.
 */

import { describe, expect, it, vi } from "vitest";
import { ComponentMetaChecker } from "./checker.js";
import type { VerterHostAdapter } from "../host-adapter.js";

function createCheckerAdapterStub(overrides: Partial<VerterHostAdapter> = {}): VerterHostAdapter {
  return {
    upsert: vi.fn(),
    getAnalysis: vi.fn(() => null),
    ...overrides,
  };
}

describe("ComponentMetaChecker.dispose", () => {
  it("closes the adapter once and prevents further use", async () => {
    const upsert = vi.fn();
    const getAnalysis = vi.fn(() => null);
    const close = vi.fn();
    const checker = new ComponentMetaChecker(
      createCheckerAdapterStub({ upsert, getAnalysis, close }),
      "/project",
    );

    checker.updateFile("Component.vue", "<template />");
    expect(upsert).toHaveBeenCalledTimes(1);

    checker.dispose();
    checker.dispose();

    expect(close).toHaveBeenCalledTimes(1);
    expect(() => checker.updateFile("Component.vue", "<template><div /></template>")).toThrow(
      /disposed/i,
    );
    await expect(checker.getComponentMeta("Component.vue")).rejects.toThrow(/disposed/i);
    expect(getAnalysis).not.toHaveBeenCalled();
  });

  it("is safe when the adapter does not expose close()", () => {
    const checker = new ComponentMetaChecker(createCheckerAdapterStub(), "/project");

    expect(() => checker.dispose()).not.toThrow();
    expect(() => checker.dispose()).not.toThrow();
  });
});
