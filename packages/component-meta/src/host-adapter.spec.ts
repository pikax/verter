/**
 * @ai-generated - Verifies native host lifecycle passthrough in the compat adapter.
 */

import { describe, expect, it, vi } from "vitest";
import { wrapNapiHost } from "./host-adapter.js";

describe("wrapNapiHost", () => {
  it("forwards close() to the wrapped native host", () => {
    const close = vi.fn();
    const adapter = wrapNapiHost({
      upsert: vi.fn(),
      getAnalysis: vi.fn(() => '{"ok":true}'),
      close,
    });

    expect(adapter.close).toBeTypeOf("function");
    expect(adapter.close).not.toBeUndefined();
    expect(adapter.getAnalysis("Component.vue")).toEqual({ ok: true });

    adapter.close?.();

    expect(close).toHaveBeenCalledTimes(1);
  });

  it("keeps close() undefined when the wrapped host does not expose it", () => {
    const adapter = wrapNapiHost({
      upsert: vi.fn(),
      getAnalysis: vi.fn(() => null),
    });

    expect(adapter.close).toBeUndefined();
    expect(adapter.getAnalysis("Missing.vue")).toBeNull();
  });
});
