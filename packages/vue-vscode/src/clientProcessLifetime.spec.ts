/** @ai-generated */
import { describe, expect, it } from "vitest";

import { clientProcessLifetimeArg } from "./clientProcessLifetime";

describe("LSP client process lifetime", () => {
  it("passes an editor-neutral client PID witness to verter-lsp", () => {
    expect(clientProcessLifetimeArg(4242)).toBe("--client-pid=4242");
  });

  it("rejects values that cannot identify a live OS process", () => {
    expect(() => clientProcessLifetimeArg(0)).toThrow("invalid editor client pid");
    expect(() => clientProcessLifetimeArg(Number.NaN)).toThrow("invalid editor client pid");
  });
});
